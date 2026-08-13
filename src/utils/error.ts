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
    const msg = (e as ErrorResponse).message;
    if (typeof msg === "string" && msg.length > 0) {
      return msg;
    }
  }
  return String(e);
}
