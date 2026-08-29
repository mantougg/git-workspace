//! Undo planning: compute the reverse action plus a live safety check for
//! every logged item, and the read-only preview built from it. Previewing
//! never modifies Git (§4.6) and never touches the DB — pure repo IO, so the
//! DB lock is never held across it. Execution reuses `plan_item` as its
//! pre-run re-check (see undo_execute).

use std::path::Path;

use rayon::prelude::*;

use super::{
    OperationLogDetail, OperationLogItem, UndoPreviewItem, OP_CHECKOUT_ALL, OP_DELETE_BRANCH_ALL,
    OP_REBASE, OP_RESET,
};

pub(super) fn repo_name_of(repo_path: &str) -> String {
    Path::new(repo_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

pub(super) fn short_oid(oid: &str) -> &str {
    &oid[..7.min(oid.len())]
}

/// Compute the undo plan (reverse action + live safety check) of every item.
/// Parallel over repos; pure repo IO, no DB.
pub fn preview_undo(detail: &OperationLogDetail) -> Vec<UndoPreviewItem> {
    detail
        .items
        .par_iter()
        .map(|item| {
            let plan = plan_item(&detail.op_type, item);
            UndoPreviewItem {
                item_id: item.id,
                repo_path: item.repo_path.clone(),
                repo_name: repo_name_of(&item.repo_path),
                action: plan.action,
                ok: plan.check.is_ok(),
                message: plan.check.err().unwrap_or_default(),
                undone: item.undone_at.is_some(),
            }
        })
        .collect()
}

pub(super) struct UndoPlan {
    pub(super) action: String,
    /// Ok(()) when the reverse op may run; Err(reason) when unsafe / moot.
    pub(super) check: Result<(), String>,
}

pub(super) fn plan_item(op_type: &str, item: &OperationLogItem) -> UndoPlan {
    if item.undone_at.is_some() {
        return UndoPlan {
            action: String::new(),
            check: Err("已撤销".to_string()),
        };
    }
    match op_type {
        OP_CHECKOUT_ALL => plan_checkout_undo(item),
        OP_DELETE_BRANCH_ALL => plan_delete_undo(item),
        OP_RESET => plan_ref_rollback(item, false),
        OP_REBASE => plan_ref_rollback(item, true),
        other => UndoPlan {
            action: String::new(),
            check: Err(format!("操作类型 '{}' 不支持撤销", other)),
        },
    }
}

/// Checkout All undo: switch back to the ref recorded before the batch op.
fn plan_checkout_undo(item: &OperationLogItem) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let action = if item.ref_name.is_empty() {
        format!("恢复分离 HEAD 到 {}", short_oid(&item.before_oid))
    } else {
        format!(
            "切回分支 '{}'（{}）",
            item.ref_name,
            short_oid(&item.before_oid)
        )
    };
    let check = (|| -> Result<(), String> {
        let repo =
            git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if item.ref_name.is_empty() {
            // Detached-before case: the commit must still exist.
            let oid =
                git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
            repo.find_commit(oid)
                .map_err(|_| "操作前提交已不存在（可能已被 GC）".to_string())?;
            let head_oid = repo.head().ok().and_then(|h| h.target());
            if head_oid == Some(oid) {
                return Err("HEAD 已处于操作前提交，无需撤销".to_string());
            }
        } else {
            repo.find_branch(&item.ref_name, git2::BranchType::Local)
                .map_err(|_| format!("原分支 '{}' 已不存在，无法切回", item.ref_name))?;
            let current = repo
                .head()
                .ok()
                .filter(|h| h.is_branch())
                .and_then(|h| h.shorthand().map(String::from));
            if current.as_deref() == Some(item.ref_name.as_str()) {
                return Err("已处于原分支，无需撤销".to_string());
            }
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Delete Branch All undo: recreate the branch at its recorded tip.
fn plan_delete_undo(item: &OperationLogItem) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let action = format!(
        "重建分支 '{}' → {}",
        item.ref_name,
        short_oid(&item.before_oid)
    );
    let check = (|| -> Result<(), String> {
        let repo =
            git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if repo
            .find_branch(&item.ref_name, git2::BranchType::Local)
            .is_ok()
        {
            return Err(format!("分支 '{}' 已存在（可能已人工重建）", item.ref_name));
        }
        let oid =
            git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
        repo.find_commit(oid)
            .map_err(|_| "删除前的分支提交已不存在（可能已被 GC）".to_string())?;
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Reset / Rebase undo: roll the branch ref back to the recorded before-oid.
/// Hard-mode rollbacks additionally require a clean worktree; rebase undo is
/// always hard (the rebase itself required a clean tree). Safety rule (§46
/// 可恢复): refuse when the branch has moved on from the recorded after-oid —
/// undoing would silently discard later commits.
fn plan_ref_rollback(item: &OperationLogItem, is_rebase: bool) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let mode = if is_rebase {
        "hard".to_string()
    } else {
        reset_mode(item.detail.as_deref())
    };
    let action = if is_rebase {
        format!(
            "将分支 '{}' 硬回退到 rebase 前 {}",
            item.ref_name,
            short_oid(&item.before_oid)
        )
    } else {
        format!(
            "将分支 '{}' 回退到 {}（reset --{}）",
            item.ref_name,
            short_oid(&item.before_oid),
            mode
        )
    };
    let check = (|| -> Result<(), String> {
        let after = item
            .after_oid
            .as_deref()
            .ok_or_else(|| "缺少操作后快照，无法校验当前状态，拒绝撤销".to_string())?;
        let repo =
            git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        // An in-progress rebase has its own recovery (rebase_abort); rolling
        // refs underneath it would corrupt that state.
        if crate::core::rebase::get_rebase_state(path)
            .ok()
            .flatten()
            .is_some()
        {
            return Err("仓库存在进行中的 rebase，请先 continue / abort".to_string());
        }
        let head = repo
            .head()
            .map_err(|e| format!("HEAD 异常：{}", e.message()))?;
        if !item.ref_name.is_empty() {
            let current = if head.is_branch() {
                head.shorthand().unwrap_or_default()
            } else {
                ""
            };
            if current != item.ref_name {
                return Err(format!(
                    "当前分支 '{}' 与记录的分支 '{}' 不符，拒绝撤销",
                    current, item.ref_name
                ));
            }
        }
        let tip = head
            .target()
            .ok_or_else(|| "HEAD 未指向提交".to_string())?
            .to_string();
        if tip != after {
            return Err(format!(
                "已有后续变更（当前 {} ≠ 操作后 {}），拒绝自动撤销",
                short_oid(&tip),
                short_oid(after)
            ));
        }
        let before =
            git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
        repo.find_commit(before)
            .map_err(|_| "操作前提交已不存在（可能已被 GC）".to_string())?;
        if tip == item.before_oid {
            return Err("分支已处于操作前状态，无需撤销".to_string());
        }
        if mode == "hard" && worktree_dirty(&repo) {
            return Err("工作区存在未提交变更，硬回退会丢失，已拒绝".to_string());
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Parse the recorded reset mode from the item detail ("mode:hard" etc.);
/// defaults to mixed (never touches the worktree) when unknown.
pub(super) fn reset_mode(detail: Option<&str>) -> String {
    match detail.and_then(|d| d.strip_prefix("mode:")) {
        Some("soft") => "soft".to_string(),
        Some("hard") => "hard".to_string(),
        _ => "mixed".to_string(),
    }
}

/// Conservative dirty probe: any tracked modification or untracked file
/// counts; on error assume dirty (fail-safe).
fn worktree_dirty(repo: &git2::Repository) -> bool {
    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true);
    repo.statuses(Some(&mut opts))
        .map(|s| !s.is_empty())
        .unwrap_or(true)
}
