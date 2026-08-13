// 从 Tauri command 的 reject 值中提取可读错误消息。
// 后端（Rust）现在返回结构化错误对象：
// { code, message, repository, operation, details, recoverable }
export interface ErrorResponse {
  code?: string;
  message?: string;
  repository?: string | null;
  operation?: string | null;
  details?: string | null;
  recoverable?: boolean;
}

/** 把 Tauri 错误（字符串或结构化对象）转成可读消息。 */
export function errMsg(e: unknown): string {
  if (typeof e === "string") {
    return e;
  }
  if (e && typeof e === "object" && "message" in e) {
    const err = e as ErrorResponse;
    const msg = err.message;
    if (typeof msg === "string" && msg.length > 0) {
      // 附加 recoverable 提示，让用户知道能否自行重试。
      if (err.recoverable === true) {
        return `${msg}（可重试）`;
      }
      if (err.recoverable === false) {
        return `${msg}（需手动处理）`;
      }
      return msg;
    }
  }
  return String(e);
}
