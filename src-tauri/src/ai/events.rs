//! 流式事件契约（设计文档缺口，AI-02 补齐；§7.2 / §16.1 / §12.1）。
//!
//! Gateway 在每次生命周期阶段迁移与每个流式 chunk 时推送 Tauri 事件
//! `ai-request://progress`；前端按帧合并（coalesce）渲染，不每 token
//! 重渲染（§16.1）。事件 payload 是 IPC 单一事实来源的一部分，进
//! golden 快照（`models/ipc_golden`）。
//!
//! 事件只承载计量与 chunk，**不含 Prompt 原文以外的敏感内容**；chunk
//! 本身是模型输出（经用户确认发送后返回的内容）。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::lifecycle::RequestPhase;

/// Tauri 事件名。
pub const AI_REQUEST_EVENT: &str = "ai-request://progress";

/// 归一化流式 chunk（§7.2：各协议事件统一映射为内部 chunk）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AiStreamChunk {
    TextDelta { text: String },
    End {
        #[serde(rename = "finishReason")]
        finish_reason: Option<String>,
    },
}

/// 事件 payload：生命周期状态 + 可选 chunk + 输出计量。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiRequestEvent {
    pub request_id: String,
    pub phase: RequestPhase,
    /// 流式 chunk（仅 Streaming 阶段携带）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk: Option<AiStreamChunk>,
    /// 已累计输出的字符数（诊断用，不含内容本身）。
    pub output_chars: i64,
}

/// 事件出口抽象。生产实现推 Tauri 事件；测试用捕获实现断言事件序列。
pub trait AiEventSink: Send + Sync {
    fn emit(&self, event: &AiRequestEvent);
}

/// 生产实现：转发到 Tauri 全局事件。
pub struct TauriAiEventSink {
    handle: AppHandle,
}

impl TauriAiEventSink {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

impl AiEventSink for TauriAiEventSink {
    fn emit(&self, event: &AiRequestEvent) {
        if let Err(e) = self.handle.emit(AI_REQUEST_EVENT, event) {
            log::warn!("ai request event emit failed: {}", e);
        }
    }
}

/// 空实现（未装配 Tauri 的场景 / 默认）。
pub struct NoopAiEventSink;

impl AiEventSink for NoopAiEventSink {
    fn emit(&self, _event: &AiRequestEvent) {}
}
