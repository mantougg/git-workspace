//! 请求生命周期状态机（设计文档 §7.3）。
//!
//! ```text
//! Created → ContextBuilding → SecretScanning → PreviewRequired → UserApproved
//!         → Queued → Sending → Streaming/Parsing → Succeeded
//! 任意非终止阶段可进入：Cancelled / Rejected / Failed / Degraded
//! ```
//!
//! Preview 闸门：`PreviewRequired → UserApproved` 之外的任何路径都不允许
//! 进入 `Queued`/`Sending`——Gateway 在 `approve()` 之外没有触发网络请求的
//! 入口，测试以「submit 后 zero 网络调用」断言（§7.3）。

use serde::{Deserialize, Serialize};

use super::error::AiError;

/// 请求生命周期阶段（§7.3）。序列化为 camelCase 字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestPhase {
    Created,
    ContextBuilding,
    SecretScanning,
    PreviewRequired,
    UserApproved,
    Queued,
    Sending,
    Streaming,
    Parsing,
    Succeeded,
    Cancelled,
    Rejected,
    Failed,
    Degraded,
}

impl RequestPhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            RequestPhase::Created => "created",
            RequestPhase::ContextBuilding => "contextBuilding",
            RequestPhase::SecretScanning => "secretScanning",
            RequestPhase::PreviewRequired => "previewRequired",
            RequestPhase::UserApproved => "userApproved",
            RequestPhase::Queued => "queued",
            RequestPhase::Sending => "sending",
            RequestPhase::Streaming => "streaming",
            RequestPhase::Parsing => "parsing",
            RequestPhase::Succeeded => "succeeded",
            RequestPhase::Cancelled => "cancelled",
            RequestPhase::Rejected => "rejected",
            RequestPhase::Failed => "failed",
            RequestPhase::Degraded => "degraded",
        }
    }

    /// 终止态：Succeeded / Cancelled / Rejected / Failed / Degraded。
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            RequestPhase::Succeeded
                | RequestPhase::Cancelled
                | RequestPhase::Rejected
                | RequestPhase::Failed
                | RequestPhase::Degraded
        )
    }

    /// §7.3 迁移表。终止态不可迁出。
    pub fn can_transition(from: RequestPhase, to: RequestPhase) -> bool {
        use RequestPhase::*;
        if from.is_terminal() {
            return false;
        }
        match (from, to) {
            (Created, ContextBuilding) => true,
            (ContextBuilding, SecretScanning) => true,
            (SecretScanning, PreviewRequired) => true,
            (PreviewRequired, UserApproved) => true,
            (UserApproved, Queued) => true,
            (Queued, Sending) => true,
            (Sending, Streaming) | (Sending, Parsing) => true,
            (Streaming, Parsing) | (Streaming, Succeeded) => true,
            (Parsing, Succeeded) => true,
            // 任意非终止阶段可进入异常终止态（§7.3）。
            (_, Cancelled) | (_, Rejected) | (_, Failed) | (_, Degraded) => true,
            _ => false,
        }
    }
}

/// 非法迁移（内部不变量被破坏时才会出现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidTransition {
    pub from: RequestPhase,
    pub to: RequestPhase,
}

/// 阶段推进器：只接受 §7.3 表内的迁移。
#[derive(Debug, Clone)]
pub struct Lifecycle {
    phase: RequestPhase,
}

impl Lifecycle {
    pub fn new() -> Self {
        Self {
            phase: RequestPhase::Created,
        }
    }

    pub fn phase(&self) -> RequestPhase {
        self.phase
    }

    pub fn is_terminal(&self) -> bool {
        self.phase.is_terminal()
    }

    /// 迁移到 `to`；非法迁移返回 `InvalidTransition`，不改变状态。
    pub fn transition(&mut self, to: RequestPhase) -> Result<RequestPhase, InvalidTransition> {
        if RequestPhase::can_transition(self.phase, to) {
            self.phase = to;
            Ok(to)
        } else {
            Err(InvalidTransition { from: self.phase, to })
        }
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

/// 把非法迁移（内部不变量破坏）转换为可上报的 AiError。
pub fn invalid_transition_error(e: InvalidTransition) -> AiError {
    AiError::ResponseInvalid {
        message: format!("非法生命周期迁移 {} -> {}", e.from.as_str(), e.to.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_walks_design_sequence() {
        let mut lc = Lifecycle::new();
        assert_eq!(lc.phase(), RequestPhase::Created);
        for step in [
            RequestPhase::ContextBuilding,
            RequestPhase::SecretScanning,
            RequestPhase::PreviewRequired,
            RequestPhase::UserApproved,
            RequestPhase::Queued,
            RequestPhase::Sending,
            RequestPhase::Streaming,
            RequestPhase::Parsing,
            RequestPhase::Succeeded,
        ] {
            lc.transition(step).unwrap();
        }
        assert_eq!(lc.phase(), RequestPhase::Succeeded);
        assert!(lc.is_terminal());
    }

    #[test]
    fn streaming_may_finish_without_parsing() {
        let mut lc = Lifecycle::new();
        for step in [
            RequestPhase::ContextBuilding,
            RequestPhase::SecretScanning,
            RequestPhase::PreviewRequired,
            RequestPhase::UserApproved,
            RequestPhase::Queued,
            RequestPhase::Sending,
            RequestPhase::Streaming,
        ] {
            lc.transition(step).unwrap();
        }
        lc.transition(RequestPhase::Succeeded).unwrap();
        assert!(lc.is_terminal());
    }

    #[test]
    fn all_terminal_states_are_reachable_and_absorbing() {
        // 任意非终止阶段可进入异常终止态（§7.3），以 ContextBuilding 采样；
        // Succeeded 只能从 Streaming/Parsing 进入，不在此列。
        for terminal in [
            RequestPhase::Cancelled,
            RequestPhase::Rejected,
            RequestPhase::Failed,
            RequestPhase::Degraded,
        ] {
            let mut lc = Lifecycle::new();
            lc.transition(RequestPhase::ContextBuilding).unwrap();
            lc.transition(terminal).unwrap();
            assert!(lc.is_terminal());
            // 终止态不可迁出（吸收性）。
            assert!(lc.transition(RequestPhase::Succeeded).is_err());
            assert!(lc.transition(RequestPhase::Failed).is_err());
            assert!(lc.transition(RequestPhase::Created).is_err());
        }
    }

    #[test]
    fn preview_gate_forbids_skipping_approval() {
        // 未确认 Preview 不允许进入 UserApproved/Queued/Sending（§7.3 闸门）。
        for illegal in [
            RequestPhase::UserApproved,
            RequestPhase::Queued,
            RequestPhase::Sending,
            RequestPhase::Streaming,
        ] {
            let mut lc = Lifecycle::new();
            lc.transition(RequestPhase::ContextBuilding).unwrap();
            lc.transition(RequestPhase::SecretScanning).unwrap();
            assert!(
                lc.transition(illegal).is_err(),
                "phase {} must not be reachable before approval",
                illegal.as_str()
            );
            // 阶段未被非法迁移改变
            assert_eq!(lc.phase(), RequestPhase::SecretScanning);
        }
    }

    #[test]
    fn reverse_and_out_of_order_transitions_are_rejected() {
        let mut lc = Lifecycle::new();
        lc.transition(RequestPhase::ContextBuilding).unwrap();
        assert!(lc.transition(RequestPhase::Created).is_err(), "不可回退");
        assert!(lc.transition(RequestPhase::Sending).is_err(), "不可跨阶段跳跃");
        assert_eq!(lc.phase(), RequestPhase::ContextBuilding);
    }

    #[test]
    fn serde_names_are_camel_case() {
        assert_eq!(
            serde_json::to_value(RequestPhase::PreviewRequired).unwrap(),
            "previewRequired"
        );
        assert_eq!(
            serde_json::to_value(RequestPhase::SecretScanning).unwrap(),
            "secretScanning"
        );
        assert_eq!(serde_json::to_value(RequestPhase::Queued).unwrap(), "queued");
    }
}
