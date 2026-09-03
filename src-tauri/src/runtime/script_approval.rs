//! Pre/Post Build Script 确认状态（R-14 §75 Command Safety）。
//!
//! 规则：**默认禁止自动执行 shell script**；首次执行必须用户确认；确认状态
//! 持久化（可撤销——「不再询问」可重置）；脚本内容变更（哈希变化）后需
//! 重新确认。
//!
//! 落点：机器级偏好文件 `<app_data_dir>/script-approvals.json`（先例：
//! `runtime-scheduler.json` / `health-weights.json`），原子写，serde default
//! 容错缺字段。

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::runtime::config::write_json_atomic;

/// 单条脚本确认记录（跨 IPC 到 UI，§80 确认状态可管理/可重置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptApproval {
    pub workspace_id: i64,
    pub runtime_name: String,
    /// `"pre"` / `"post"`。
    pub script_type: String,
    /// 脚本内容哈希：内容变更后需重新确认。
    pub script_hash: String,
    /// 脚本预览（首行截断），供 UI 展示。
    pub preview: String,
    pub approved_at: String,
    /// 最近一次实际执行时间（确认后执行且记录，§75）。
    pub last_executed_at: Option<String>,
}

/// 持久化文档（向前兼容：未知字段忽略）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApprovalFile {
    #[serde(default)]
    approvals: Vec<ScriptApproval>,
}

/// 脚本确认状态存取。无内部可变状态：每次操作读文件 + 原子写回。
#[derive(Debug, Clone)]
pub struct ScriptApprovalStore {
    path: PathBuf,
}

/// 生产路径（`<app_data_dir>/script-approvals.json`）。
pub fn script_approvals_path() -> PathBuf {
    crate::get_app_data_dir().join("script-approvals.json")
}

/// 稳定哈希（跨进程一致：`DefaultHasher::new()` 使用固定 seed 0/0）。
/// 仅用于「内容变更识别」，不用于安全校验。
pub fn script_hash(script: &str) -> String {
    let mut hasher = DefaultHasher::new();
    script.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// 预览：首行 + 字符数截断（UI 展示 + 可追溯；多字节安全）。
pub fn script_preview(script: &str) -> String {
    const MAX_CHARS: usize = 200;
    let first_line = script.lines().next().unwrap_or("").trim();
    if first_line.chars().count() > MAX_CHARS {
        let head: String = first_line.chars().take(MAX_CHARS - 1).collect();
        format!("{head}…")
    } else {
        first_line.to_string()
    }
}

impl ScriptApprovalStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> ApprovalFile {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!(
                    "R-14: invalid script-approvals file {:?}: {}; starting empty",
                    self.path,
                    e
                );
                ApprovalFile::default()
            }),
            Err(_) => ApprovalFile::default(),
        }
    }

    fn save(&self, file: &ApprovalFile) -> AppResult<()> {
        write_json_atomic(&self.path, file)
    }

    /// 全部确认记录（UI 管理列表）。
    pub fn list(&self) -> Vec<ScriptApproval> {
        self.load().approvals
    }

    /// 脚本是否已确认（key = workspace + runtime + type + 内容哈希）。
    pub fn is_approved(&self, workspace_id: i64, runtime_name: &str, script_type: &str, hash: &str) -> bool {
        self.load().approvals.iter().any(|a| {
            a.workspace_id == workspace_id
                && a.runtime_name == runtime_name
                && a.script_type == script_type
                && a.script_hash == hash
        })
    }

    /// 确认一条脚本（upsert：同 key 覆盖，刷新 approved_at）。已存在时
    /// 返回 false（表示是重复确认），首次确认返回 true。
    pub fn approve(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        script_type: &str,
        hash: &str,
        preview: &str,
    ) -> AppResult<bool> {
        let mut file = self.load();
        let existing = file.approvals.iter_mut().find(|a| {
            a.workspace_id == workspace_id
                && a.runtime_name == runtime_name
                && a.script_type == script_type
                && a.script_hash == hash
        });
        let created = existing.is_none();
        let now = Utc::now().to_rfc3339();
        if let Some(approval) = existing {
            approval.approved_at = now;
            approval.preview = preview.to_string();
        } else {
            file.approvals.push(ScriptApproval {
                workspace_id,
                runtime_name: runtime_name.to_string(),
                script_type: script_type.to_string(),
                script_hash: hash.to_string(),
                preview: preview.to_string(),
                approved_at: now,
                last_executed_at: None,
            });
        }
        self.save(&file)?;
        Ok(created)
    }

    /// 记录一次实际执行（「确认后执行且记录」）。
    pub fn record_execution(
        &self,
        workspace_id: i64,
        runtime_name: &str,
        script_type: &str,
        hash: &str,
    ) -> AppResult<()> {
        let mut file = self.load();
        let now = Utc::now().to_rfc3339();
        if let Some(approval) = file.approvals.iter_mut().find(|a| {
            a.workspace_id == workspace_id
                && a.runtime_name == runtime_name
                && a.script_type == script_type
                && a.script_hash == hash
        }) {
            approval.last_executed_at = Some(now);
            self.save(&file)?;
        }
        Ok(())
    }

    /// 按范围撤销确认（「不再询问」可重置，§75）。
    /// `workspace_id` / `runtime_name` 为 `None` 时匹配任意；全部为 `None`
    /// 即清空。返回删除条数。
    pub fn reset(&self, workspace_id: Option<i64>, runtime_name: Option<&str>) -> AppResult<usize> {
        let mut file = self.load();
        let before = file.approvals.len();
        file.approvals.retain(|a| {
            let ws_matches = workspace_id.map_or(true, |ws| a.workspace_id == ws);
            let name_matches = runtime_name.map_or(true, |name| a.runtime_name == name);
            !(ws_matches && name_matches)
        });
        let removed = before - file.approvals.len();
        if removed > 0 {
            self.save(&file)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (ScriptApprovalStore, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "gw_r14_approval_{tag}_{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        (ScriptApprovalStore::new(dir.join("approvals.json")), dir)
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        let h1 = script_hash("echo hello");
        let h2 = script_hash("echo hello");
        let h3 = script_hash("echo hello ");
        assert_eq!(h1, h2, "same content must hash identically");
        assert_ne!(h1, h3, "content change must change the hash");
    }

    #[test]
    fn unapproved_by_default_until_explicit_approval() {
        let (store, dir) = temp_store("default");
        let hash = script_hash("echo hi");
        assert!(!store.is_approved(1, "app", "pre", &hash));
        assert!(store.approve(1, "app", "pre", &hash, "echo hi").unwrap());
        assert!(store.is_approved(1, "app", "pre", &hash));
        // 不同 runtime / workspace / type / hash 均不匹配。
        assert!(!store.is_approved(2, "app", "pre", &hash));
        assert!(!store.is_approved(1, "other", "pre", &hash));
        assert!(!store.is_approved(1, "app", "post", &hash));
        assert!(!store.is_approved(1, "app", "pre", "other-hash"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn approval_persists_across_reload() {
        let (store, dir) = temp_store("persist");
        let hash = script_hash("echo hi");
        store.approve(3, "svc", "post", &hash, "echo hi").unwrap();
        // 同一路径重建 store = 模拟重启。
        let reloaded = ScriptApprovalStore::new(store.path().to_path_buf());
        assert!(reloaded.is_approved(3, "svc", "post", &hash));
        assert_eq!(reloaded.list().len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn record_execution_marks_last_run() {
        let (store, dir) = temp_store("exec");
        let hash = script_hash("echo hi");
        store.approve(1, "app", "pre", &hash, "echo hi").unwrap();
        store.record_execution(1, "app", "pre", &hash).unwrap();
        let entry = store.list().into_iter().find(|a| a.script_hash == hash).unwrap();
        assert!(entry.last_executed_at.is_some());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reset_removes_by_scope_and_is_reversible() {
        let (store, dir) = temp_store("reset");
        for (ws, name) in [(1, "app"), (1, "web"), (2, "app")] {
            let hash = script_hash(&format!("{ws}-{name}"));
            store.approve(ws, name, "pre", &hash, "").unwrap();
        }
        assert_eq!(store.reset(Some(1), None).unwrap(), 2, "workspace 1 has 2");
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.reset(None, Some("app")).unwrap(), 1, "remaining app");
        assert!(store.list().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn preview_takes_first_line_and_truncates() {
        assert_eq!(script_preview("#!/bin/sh\necho hi"), "#!/bin/sh");
        let long = "x".repeat(500);
        let preview = script_preview(&long);
        assert!(preview.chars().count() <= 200);
        assert!(preview.ends_with('…'));
    }
}
