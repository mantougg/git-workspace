// 从 Tauri command 的 reject 值中提取可读错误消息。
// 后端（Rust）现在返回结构化错误对象：
// { code, message, repository, operation, details, recoverable }
import { classifyGitErrorText, type GitErrorGuidance } from "./gitError";

export interface ErrorResponse {
  code?: string;
  message?: string;
  repository?: string | null;
  operation?: string | null;
  details?: string | null;
  recoverable?: boolean;
}

/** 解析 details 字段（JSON 字符串 → 对象；非 JSON 返回 null）。 */
export function parseErrorDetails(e: unknown): Record<string, unknown> | null {
  if (!e || typeof e !== "object" || !("details" in e)) return null;
  const raw = (e as ErrorResponse).details;
  if (typeof raw !== "string" || raw.length === 0) return null;
  try {
    const value = JSON.parse(raw);
    return value && typeof value === "object"
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function toActionList(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const items = value.filter((v): v is string => typeof v === "string");
  return items.length > 0 ? items : null;
}

/**
 * GF-08：提取错误的可行动引导（分类原因 + 建议操作）。
 *
 * 两个来源：
 * 1. 结构化 details（`src-tauri/src/error.rs` 为 Git 错误 / 平台 token
 *    缺失错误序列化的 category / reason / suggestedActions）——后端分类
 *    优先，可用 libgit2 error class 补充；
 * 2. 纯文本分类（任务失败等只带 message 字符串的路径，前端镜像后端
 *    `classify_git_error` 的模式匹配）。
 *
 * 只对 Git 域错误生效（code 为 GitError / RemoteAuthRequired，或纯文本
 * 可分分类）；AI 等其它域错误的 details 即使带 suggestedActions 也不经
 * 此函数消费。
 */
export function errorGuidance(e: unknown): GitErrorGuidance | null {
  if (typeof e === "string") return classifyGitErrorText(e);
  if (!e || typeof e !== "object") return null;
  const err = e as ErrorResponse;
  const code = err.code;
  const message = typeof err.message === "string" ? err.message : "";
  if (code === "GitError" || code === "RemoteAuthRequired") {
    // 结构化 details（后端分类，含 libgit2 class 兜底）优先。
    const details = parseErrorDetails(e);
    const actions = details ? toActionList(details.suggestedActions) : null;
    if (actions) {
      const reason =
        details && typeof details.reason === "string" ? details.reason : undefined;
      const category =
        details && typeof details.category === "string" ? details.category : undefined;
      return { category, reason, actions };
    }
    // details 缺省（未分类）时前端镜像分类兜底。
    return classifyGitErrorText(message);
  }
  return null;
}

/** GF-08：guidance → 单行可读文本（toast / 详情行用）。 */
export function formatGuidance(guidance: GitErrorGuidance): string {
  const parts: string[] = [];
  if (guidance.reason) parts.push(guidance.reason);
  if (guidance.actions.length > 0) parts.push(`建议：${guidance.actions.join("；")}`);
  return parts.join(" ");
}

/** 把 Tauri 错误（字符串或结构化对象）转成可读消息。 */
export function errMsg(e: unknown): string {
  if (typeof e === "string") {
    return appendGitGuidance(e, e);
  }
  if (e && typeof e === "object" && "message" in e) {
    const err = e as ErrorResponse;
    const msg = err.message;
    if (typeof msg === "string" && msg.length > 0) {
      // 附加 recoverable 提示，让用户知道能否自行重试。
      let suffix = "";
      if (err.recoverable === true) {
        suffix = "（可重试）";
      } else if (err.recoverable === false) {
        suffix = "（需手动处理）";
      }
      return appendGitGuidance(`${msg}${suffix}`, msg, err.code);
    }
  }
  return String(e);
}

/**
 * GF-08：Git 错误追加可行动引导（分类原因 + 建议操作）。
 *
 * 原消息作为前缀保留（信息不丢失），引导以换行附在后面；非 Git 错误
 * 或未分类的 Git 错误文案保持原样——非认证类错误展示行为不劣化。
 */
function appendGitGuidance(full: string, message: string, code?: string): string {
  const guidance = code
    ? errorGuidance({ code, message })
    : classifyGitErrorText(message);
  if (!guidance) return full;
  return `${full}\n${formatGuidance(guidance)}`;
}
