import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AiRequestEvent } from "@/types/ai";

// ---------------------------------------------------------------------------
// AI-02：流式事件契约（§7.2 / §16.1）
// ---------------------------------------------------------------------------

/** Gateway 生命周期与流式 chunk 事件（Rust `ai/events.rs` 契约）。 */
export const AI_REQUEST_EVENT = "ai-request://progress";

/**
 * 订阅指定请求（或全部请求）的流式事件。
 * 返回取消监听函数；调用方负责在作用域销毁时调用。
 */
export function onAiRequestEvent(
  handler: (event: AiRequestEvent) => void,
  requestId?: string,
): Promise<UnlistenFn> {
  return listen<AiRequestEvent>(AI_REQUEST_EVENT, (e) => {
    if (requestId && e.payload.requestId !== requestId) return;
    handler(e.payload);
  });
}

/**
 * 合帧缓冲：把高频到达的 textDelta 先进缓冲，每帧（rAF）合并一次输出，
 * 避免每 token 触发完整重渲染（§16.1）。
 */
export class AiStreamFrameBuffer {
  private pending = "";
  private frame: number | null = null;

  constructor(private readonly flush: (mergedText: string) => void) {}

  /** 追加一个增量；下一次动画帧统一冲刷。 */
  push(delta: string): void {
    this.pending += delta;
    if (this.frame !== null) return;
    this.frame = requestAnimationFrame(() => {
      this.frame = null;
      const merged = this.pending;
      this.pending = "";
      if (merged) this.flush(merged);
    });
  }

  /** 立即冲刷剩余缓冲（流结束时调用，防止最后一帧丢失）。 */
  finish(): void {
    if (this.frame !== null) {
      cancelAnimationFrame(this.frame);
      this.frame = null;
    }
    if (this.pending) {
      const merged = this.pending;
      this.pending = "";
      this.flush(merged);
    }
  }

  dispose(): void {
    this.finish();
  }
}
