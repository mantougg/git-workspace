//! Runtime 生命周期状态机（R-10，§27）。
//!
//! 特有边界：
//! - `Preparing → Starting`：Restart / skip-build 路径复用最近构建产物，
//!   跳过 Resolving/Building 直接启动（验收标准 2）。
//! - `Starting / Running / Stopping → Stopped`：进程自然退出（exit 0）与
//!   停止完成共用终态边。
//! - `Running → Failed`：运行中进程崩溃（非零退出码 → `ProcessCrashed`）。
//! - `Stopping → Failed`：停止过程中进程以异常方式结束（仍记录退出码）。
//! - `Created → Stopping`：start 刚建行即被 Stop 的竞态兜底。
//! - `Stopped` / `Failed` 为终态；重新启动创建新的进程记录而非复活旧行。

use serde::{Deserialize, Serialize};

/// 生命周期状态（§27）。序列化为小写串，与 DB `runtime_processes.status`
/// 及未来 IPC 事件载荷一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleStatus {
    Created,
    Preparing,
    Resolving,
    Building,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl LifecycleStatus {
    /// 稳定字符串（与 serde 一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleStatus::Created => "created",
            LifecycleStatus::Preparing => "preparing",
            LifecycleStatus::Resolving => "resolving",
            LifecycleStatus::Building => "building",
            LifecycleStatus::Starting => "starting",
            LifecycleStatus::Running => "running",
            LifecycleStatus::Stopping => "stopping",
            LifecycleStatus::Stopped => "stopped",
            LifecycleStatus::Failed => "failed",
        }
    }

    /// 从 DB 字符串还原；未知值返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "created" => Some(LifecycleStatus::Created),
            "preparing" => Some(LifecycleStatus::Preparing),
            "resolving" => Some(LifecycleStatus::Resolving),
            "building" => Some(LifecycleStatus::Building),
            "starting" => Some(LifecycleStatus::Starting),
            "running" => Some(LifecycleStatus::Running),
            "stopping" => Some(LifecycleStatus::Stopping),
            "stopped" => Some(LifecycleStatus::Stopped),
            "failed" => Some(LifecycleStatus::Failed),
            _ => None,
        }
    }

    /// 终态：Stopped / Failed。终态行不再接受任何迁移。
    pub fn is_terminal(self) -> bool {
        matches!(self, LifecycleStatus::Stopped | LifecycleStatus::Failed)
    }

    /// 是否处于「活跃」状态（介于已开始与终态之间，含 Stopping）。
    /// reconcile / 列表高亮用。
    pub fn is_active(self) -> bool {
        !self.is_terminal() && self != LifecycleStatus::Created
    }

    /// 合法迁移表。见模块文档的边说明。
    pub fn can_transition(self, to: LifecycleStatus) -> bool {
        use LifecycleStatus::*;
        if self.is_terminal() {
            return false;
        }
        if to == Failed {
            // 任意非终态可因异常进 Failed。
            return self != Failed;
        }
        match (self, to) {
            (Created, Preparing) => true,
            // Created → Stopping：start 刚建行即被 Stop 的竞态兜底。
            (Created, Stopping) => true,
            (Preparing, Resolving | Starting | Stopping) => true,
            (Resolving, Building | Stopping) => true,
            (Building, Starting | Stopping) => true,
            (Starting, Running | Stopping) => true,
            (Running, Stopping) => true,
            // 自然退出（exit 0）或停止完成：活跃态可直接落 Stopped。
            (Starting | Running | Stopping, Stopped) => true,
            _ => false,
        }
    }

    /// 执行迁移；非法迁移返回 `RuntimeConfig` 错误（调用方 bug，非用户错误，
    /// 但经统一错误体系上报，便于定位）。
    pub fn transition(self, to: LifecycleStatus) -> crate::error::AppResult<LifecycleStatus> {
        if self.can_transition(to) {
            Ok(to)
        } else {
            Err(crate::error::AppError::RuntimeConfig(format!(
                "非法的 Runtime 生命周期迁移：{} → {}",
                self.as_str(),
                to.as_str()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_roundtrip() {
        for status in [
            LifecycleStatus::Created,
            LifecycleStatus::Preparing,
            LifecycleStatus::Resolving,
            LifecycleStatus::Building,
            LifecycleStatus::Starting,
            LifecycleStatus::Running,
            LifecycleStatus::Stopping,
            LifecycleStatus::Stopped,
            LifecycleStatus::Failed,
        ] {
            assert_eq!(LifecycleStatus::parse(status.as_str()), Some(status));
            assert_eq!(
                serde_json::to_value(status).unwrap(),
                serde_json::json!(status.as_str())
            );
        }
        assert_eq!(LifecycleStatus::parse("bogus"), None);
    }

    #[test]
    fn happy_path_walks_the_full_chain() {
        let mut status = LifecycleStatus::Created;
        for next in [
            LifecycleStatus::Preparing,
            LifecycleStatus::Resolving,
            LifecycleStatus::Building,
            LifecycleStatus::Starting,
            LifecycleStatus::Running,
            LifecycleStatus::Stopping,
            LifecycleStatus::Stopped,
        ] {
            status = status.transition(next).unwrap();
        }
        assert!(status.is_terminal());
    }

    #[test]
    fn skip_build_path_jumps_from_preparing_to_starting() {
        let status = LifecycleStatus::Created
            .transition(LifecycleStatus::Preparing)
            .unwrap()
            .transition(LifecycleStatus::Starting)
            .unwrap()
            .transition(LifecycleStatus::Running)
            .unwrap();
        assert_eq!(status, LifecycleStatus::Running);
    }

    #[test]
    fn any_non_terminal_state_can_fail_but_terminals_are_frozen() {
        use LifecycleStatus::*;
        for from in [Created, Preparing, Resolving, Building, Starting, Running, Stopping] {
            assert!(from.can_transition(Failed), "{from:?} → Failed must be legal");
        }
        for terminal in [Stopped, Failed] {
            assert!(terminal.is_terminal());
            for to in [Created, Preparing, Running, Stopping, Stopped, Failed] {
                assert!(!terminal.can_transition(to), "{terminal:?} must be frozen");
            }
        }
    }

    #[test]
    fn natural_exit_edges_reach_stopped_from_active_states() {
        use LifecycleStatus::*;
        for from in [Starting, Running, Stopping] {
            assert!(from.can_transition(Stopped), "{from:?} → Stopped (natural exit)");
        }
        // Created 行可被 Stop 竞态兜底，但不能直接落终态。
        assert!(Created.can_transition(Stopping));
        assert!(!Created.can_transition(Stopped));
    }

    #[test]
    fn illegal_jumps_are_rejected() {
        use LifecycleStatus::*;
        for (from, to) in [
            (Created, Running),
            (Created, Building),
            (Resolving, Running),
            (Running, Building),
            (Stopping, Running),
            (Running, Starting),
        ] {
            assert!(!from.can_transition(to), "{from:?} → {to:?} must be illegal");
            assert!(from.transition(to).is_err());
        }
    }
}
