//! Undo planning: compute the reverse action plus a live safety check for
//! every logged item, and the read-only preview built from it. Previewing
//! never modifies Git (§4.6) and never touches the DB — pure repo IO, so the
//! DB lock is never held across it. Execution reuses `plan_item` as its
//! pre-run re-check (see undo_execute).

use std::path::Path;

use rayon::prelude::*;

use super::detail_snapshots::{decode_stash_snapshot, decode_worktree_snapshot};
use super::{
    OperationLogDetail, OperationLogItem, UndoPreviewItem, OP_AI_COMMIT, OP_CHECKOUT_ALL, OP_CHERRY_PICK,
    OP_CONFLICT_RESOLUTION, OP_CREATE_BRANCH_ALL, OP_DELETE_BRANCH_ALL, OP_MERGE_ABORT, OP_REBASE, OP_RESET,
    OP_RESTORE_FILES, OP_STASH_CLEAR, OP_STASH_DROP, OP_WORKTREE_REMOVE,
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

/// Char-bounded excerpt of a stash reflog message for preview lines.
fn excerpt(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
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
        OP_CREATE_BRANCH_ALL => plan_create_undo(item),
        OP_RESET => plan_ref_rollback(item, false),
        OP_REBASE => plan_ref_rollback(item, true),
        OP_AI_COMMIT => plan_ref_rollback(item, false),
        OP_CHERRY_PICK => plan_cherry_pick_undo(item),
        OP_MERGE_ABORT => plan_merge_abort_undo(item),
        OP_WORKTREE_REMOVE => plan_worktree_restore(item),
        OP_STASH_DROP | OP_STASH_CLEAR => plan_stash_restore(item),
        // Logged for traceability only: what they discarded lives outside
        // the ref-snapshot model (file contents / worktree edits), so undo
        // refuses with an explicit reason instead of "unsupported".
        OP_CONFLICT_RESOLUTION | OP_RESTORE_FILES => plan_not_recoverable(op_type),
        other => UndoPlan {
            action: String::new(),
            check: Err(format!("操作类型 '{}' 不支持撤销", other)),
        },
    }
}

/// Create Branch All undo: delete the created branch again — but only while
/// it still sits at the recorded tip (a branch that gained commits since is
/// the user's work, not ours to delete).
fn plan_create_undo(item: &OperationLogItem) -> UndoPlan {
    let action = item
        .after_oid
        .as_deref()
        .map(|a| format!("删除新建分支 '{}'（创建于 {}）", item.ref_name, short_oid(a)))
        .unwrap_or_default();
    let check = (|| -> Result<(), String> {
        let after = item
            .after_oid
            .as_deref()
            .ok_or_else(|| "尚缺操作后快照（后台回填未完成或创建失败），请稍后重试".to_string())?;
        let repo = git2::Repository::open(Path::new(&item.repo_path))
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        let branch = repo
            .find_branch(&item.ref_name, git2::BranchType::Local)
            .map_err(|_| format!("分支 '{}' 不存在（可能创建失败或已人工删除），无需撤销", item.ref_name))?;
        let tip = branch
            .get()
            .target()
            .ok_or_else(|| "分支引用异常".to_string())?
            .to_string();
        if tip != after {
            return Err(format!(
                "分支已有新提交（当前 {} ≠ 创建时 {}），拒绝撤销删除",
                short_oid(&tip),
                short_oid(after)
            ));
        }
        if branch.is_head() {
            return Err(format!("分支 '{}' 是当前分支，请先切换再撤销", item.ref_name));
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Ops that are deliberately not undoable (GF-16 checklist 4): the log row
/// exists for traceability, but the reverse state cannot be reconstructed
/// from ref snapshots — say so, plus how to recover manually.
fn plan_not_recoverable(op_type: &str) -> UndoPlan {
    let reason = if op_type == OP_CONFLICT_RESOLUTION {
        "冲突解决不可自动撤销：文件内容不在 ref 快照内；如需恢复请使用当前操作的 Abort 流程或手动编辑".to_string()
    } else {
        "批量 restore 不可自动撤销：丢弃的工作区改动不在 ref 快照内；未提交内容可用 reflog / stash 保底".to_string()
    };
    UndoPlan {
        action: "不可自动撤销".to_string(),
        check: Err(reason),
    }
}

/// Cherry-pick undo: hard rollback to the pre-pick oid (the pick applied
/// cleanly, so the branch simply moves back). Refused while another
/// cherry-pick / revert is in progress — that flow owns recovery.
fn plan_cherry_pick_undo(item: &OperationLogItem) -> UndoPlan {
    let action = format!(
        "将分支 '{}' 硬回退到 cherry-pick 前 {}",
        item.ref_name,
        short_oid(&item.before_oid)
    );
    let check = (|| -> Result<(), String> {
        let repo = git2::Repository::open(Path::new(&item.repo_path))
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        let gitdir = repo.path();
        if gitdir.join("CHERRY_PICK_HEAD").exists() || gitdir.join("REVERT_HEAD").exists() {
            return Err("仓库存在进行中的 cherry-pick / revert，请先 continue / abort".to_string());
        }
        Ok(())
    })();
    if let Err(e) = check {
        return UndoPlan { action, check: Err(e) };
    }
    let plan = plan_ref_rollback(item, true);
    UndoPlan { action, check: plan.check }
}

/// Merge-abort undo: re-run the merge toward the recorded MERGE_HEAD, which
/// deterministically restores the conflict state the abort discarded. Refused
/// when HEAD moved on since the abort (the re-merge would produce a different
/// result) or another operation is in progress.
fn plan_merge_abort_undo(item: &OperationLogItem) -> UndoPlan {
    let merge_head = item.detail.as_deref().and_then(|d| d.strip_prefix("mergehead:"));
    let action = merge_head
        .map(|m| format!("重新合并 {} 到 '{}'（恢复冲突状态）", short_oid(m), item.ref_name))
        .unwrap_or_default();
    let check = (|| -> Result<(), String> {
        let after = item
            .after_oid
            .as_deref()
            .ok_or_else(|| "缺少操作后快照，无法校验当前状态，拒绝撤销".to_string())?;
        let merge_head = merge_head.ok_or_else(|| "缺少 MERGE_HEAD 记录，无法重新合并".to_string())?;
        let repo = git2::Repository::open(Path::new(&item.repo_path))
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if repo.path().join("MERGE_HEAD").exists() {
            return Err("已有 merge 进行中，请先继续或中止".to_string());
        }
        if crate::core::rebase::get_rebase_state(Path::new(&item.repo_path))
            .ok()
            .flatten()
            .is_some()
        {
            return Err("仓库存在进行中的 rebase，请先 continue / abort".to_string());
        }
        let head = repo.head().map_err(|e| format!("HEAD 异常：{}", e.message()))?;
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
        let tip = head.target().ok_or_else(|| "HEAD 未指向提交".to_string())?.to_string();
        if tip != after {
            return Err(format!(
                "已有后续变更（当前 {} ≠ 操作后 {}），拒绝重新合并",
                short_oid(&tip),
                short_oid(after)
            ));
        }
        let oid = git2::Oid::from_str(merge_head).map_err(|_| "记录的 MERGE_HEAD oid 无效".to_string())?;
        repo.find_commit(oid)
            .map_err(|_| "原合并目标提交已不存在（可能已被 GC）".to_string())?;
        if worktree_dirty(&repo) {
            return Err("工作区存在未提交变更，重新合并前请先处理".to_string());
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Worktree-remove undo: recreate the worktree at its recorded position.
/// The branch must still sit at the recorded tip; the path must be free.
/// Uncommitted changes of the removed worktree are NOT recoverable — they
/// were part of the deletion.
fn plan_worktree_restore(item: &OperationLogItem) -> UndoPlan {
    let snap = decode_worktree_snapshot(item.detail.as_deref());
    let action = snap
        .as_ref()
        .map(|s| format!("重建 worktree '{}'（{}）", s.name, s.path))
        .unwrap_or_default();
    let check = (|| -> Result<(), String> {
        let snap = snap.ok_or_else(|| "缺少 worktree 快照记录，无法重建".to_string())?;
        let wt_path = Path::new(&snap.path);
        if wt_path.exists() && wt_path.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
            return Err(format!("原路径已存在内容：{}", snap.path));
        }
        let repo = git2::Repository::open(Path::new(&item.repo_path))
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if repo.find_worktree(&snap.name).is_ok() {
            return Err(format!("worktree '{}' 已存在，无需重建", snap.name));
        }
        if let Some(branch) = &snap.branch {
            let b = repo
                .find_branch(branch, git2::BranchType::Local)
                .map_err(|_| format!("分支 '{branch}' 已不存在，无法恢复 worktree 检出"))?;
            let tip = b
                .get()
                .target()
                .ok_or_else(|| "分支引用异常".to_string())?
                .to_string();
            if tip != item.before_oid {
                return Err(format!(
                    "分支已有新提交（当前 {} ≠ 移除时 {}），拒绝恢复",
                    short_oid(&tip),
                    short_oid(&item.before_oid)
                ));
            }
        } else {
            let oid_str = snap
                .oid
                .ok_or_else(|| "worktree 快照缺少 detached oid".to_string())?;
            let oid = git2::Oid::from_str(&oid_str).map_err(|_| "记录的 oid 无效".to_string())?;
            repo.find_commit(oid)
                .map_err(|_| "原提交已不存在（可能已被 GC）".to_string())?;
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Stash drop / clear undo: restore the recorded stack entries into
/// `refs/stash`. The stash commits survive the drop (only the reflog entry
/// is removed), so this is exact unless GC pruned them — every recorded oid
/// is verified here. Entries already back on the stack make it a no-op.
fn plan_stash_restore(item: &OperationLogItem) -> UndoPlan {
    let entries = decode_stash_snapshot(item.detail.as_deref());
    let action = if entries.is_empty() {
        String::new()
    } else {
        format!(
            "恢复 {} 条 stash 记录（最近：{}）",
            entries.len(),
            excerpt(&entries[0].1, 24)
        )
    };
    let check = (|| -> Result<(), String> {
        if entries.is_empty() {
            return Err("缺少 stash 快照记录，无法恢复".to_string());
        }
        let mut repo = git2::Repository::open(Path::new(&item.repo_path))
            .map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        for (oid, _) in &entries {
            let oid = git2::Oid::from_str(oid).map_err(|_| "记录的 stash oid 无效".to_string())?;
            repo.find_commit(oid)
                .map_err(|_| format!("stash 提交 {} 已不存在（可能已被 GC）", short_oid(&oid.to_string())))?;
        }
        let mut present = 0usize;
        let _ = repo.stash_foreach(|_, _, oid| {
            if entries.iter().any(|(e, _)| e == &oid.to_string()) {
                present += 1;
            }
            true
        });
        if present == entries.len() {
            return Err("记录的 stash 已在栈中，无需撤销".to_string());
        }
        Ok(())
    })();
    UndoPlan { action, check }
}

/// Checkout All undo: switch back to the ref recorded before the batch op.
fn plan_checkout_undo(item: &OperationLogItem) -> UndoPlan {
    let path = Path::new(&item.repo_path);
    let action = if item.ref_name.is_empty() {
        format!("恢复分离 HEAD 到 {}", short_oid(&item.before_oid))
    } else {
        format!("切回分支 '{}'（{}）", item.ref_name, short_oid(&item.before_oid))
    };
    let check = (|| -> Result<(), String> {
        let repo = git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if item.ref_name.is_empty() {
            // Detached-before case: the commit must still exist.
            let oid = git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
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
    let action = format!("重建分支 '{}' → {}", item.ref_name, short_oid(&item.before_oid));
    let check = (|| -> Result<(), String> {
        let repo = git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        if repo.find_branch(&item.ref_name, git2::BranchType::Local).is_ok() {
            return Err(format!("分支 '{}' 已存在（可能已人工重建）", item.ref_name));
        }
        let oid = git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
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
        let repo = git2::Repository::open(path).map_err(|e| format!("仓库无法打开：{}", e.message()))?;
        // An in-progress rebase has its own recovery (rebase_abort); rolling
        // refs underneath it would corrupt that state.
        if crate::core::rebase::get_rebase_state(path).ok().flatten().is_some() {
            return Err("仓库存在进行中的 rebase，请先 continue / abort".to_string());
        }
        let head = repo.head().map_err(|e| format!("HEAD 异常：{}", e.message()))?;
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
        let tip = head.target().ok_or_else(|| "HEAD 未指向提交".to_string())?.to_string();
        if tip != after {
            return Err(format!(
                "已有后续变更（当前 {} ≠ 操作后 {}），拒绝自动撤销",
                short_oid(&tip),
                short_oid(after)
            ));
        }
        let before = git2::Oid::from_str(&item.before_oid).map_err(|_| "记录的 oid 无效".to_string())?;
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
    repo.statuses(Some(&mut opts)).map(|s| !s.is_empty()).unwrap_or(true)
}
