/**
 * GF-08：Git 错误分类（前端镜像）。
 *
 * 与后端 `src-tauri/src/error.rs::classify_git_error` 的模式一一对应，
 * 用于 details 不可达的路径（任务面板失败行 `TaskStatus::Failed.error`
 * 只是纯文本字符串）。后端分类优先时（结构化 details）不使用本文件。
 *
 * 两边改一边必须同步另一边；文案（reason / actions）同样镜像后端
 * `GitErrorCategory::reason / suggested_actions`，均为**固定文案**，
 * 不回填错误原文（防 token/密码泄漏）。
 */

export type GitErrorCategoryCode =
  | "authentication"
  | "network"
  | "lock"
  | "dirtyTree"
  | "rejected";

export interface GitErrorGuidance {
  /** 后端 details 的 category；纯文本分类时也有。 */
  category?: string;
  /** 分类原因（固定文案）。 */
  reason?: string;
  /** 建议操作（固定文案）。 */
  actions: string[];
}

const REASONS: Record<GitErrorCategoryCode, string> = {
  authentication: "认证失败：平台拒绝了当前凭据（HTTPS 令牌/密码或 SSH key）",
  network: "网络不可达：无法连接到远程平台（DNS / 连接 / 超时 / 证书）",
  lock: "仓库被锁：另一个 Git 进程可能正在使用该仓库",
  dirtyTree: "工作区有未提交改动：本地修改会被本次操作覆盖",
  rejected: "推送被拒：远端包含你没有的提交（非快进）",
};

const ACTIONS: Record<GitErrorCategoryCode, string[]> = {
  authentication: [
    "打开系统凭据管理器，更新该平台的 Git 凭据",
    "检查 SSH key 配置（~/.ssh 下的私钥与 ssh-agent）",
    "确认平台访问令牌是否已过期或被撤销",
  ],
  network: ["检查网络连接与代理设置后重试", "确认平台地址可达（防火墙 / VPN / 证书链）"],
  lock: [
    "关闭其它 Git 客户端或等待其结束后重试",
    "确认无残留 git 进程后，删除仓库 .git 目录下的锁文件",
  ],
  dirtyTree: ["先提交或 stash 本地改动，再执行远程操作", "在变更页确认需要保留的工作区改动"],
  rejected: [
    "先 pull（或 rebase）同步远端最新提交后再推送",
    "确需覆盖远端历史时使用 --force-with-lease，并先确认影响范围",
  ],
};

/** 分类 → 用户可见标签（任务面板 chip 用）。 */
export const GIT_ERROR_LABELS: Record<GitErrorCategoryCode, string> = {
  authentication: "认证失败",
  network: "网络不可达",
  lock: "仓库被锁",
  dirtyTree: "工作区有改动",
  rejected: "推送被拒",
};

const AUTH_PATTERNS = [
  "authentication failed",
  "authentication required",
  "failed to authenticate",
  "permission denied",
  "publickey",
  "access denied",
  "invalid username or password",
  "invalid credentials",
  "could not read username",
  "could not read from remote repository",
  "terminal prompts disabled",
  "unable to read askpass",
  "no password available",
  "too many authentication failures",
  "host key verification failed",
  "remote: repository not found",
  "repository not found",
  "403 forbidden",
  "401 unauthorized",
  "the requested url returned error: 401",
  "the requested url returned error: 403",
];

const NETWORK_PATTERNS = [
  "failed to connect",
  "could not connect",
  "couldn't connect",
  "could not resolve host",
  "could not resolve hostname",
  "couldn't resolve host",
  "connection refused",
  "connection reset",
  "connection timed out",
  "network is unreachable",
  "unable to access",
  "operation timed out",
  "ssl certificate problem",
  "certificate verify failed",
  "server certificate verification failed",
  "proxy connect",
  "the remote end hung up unexpectedly",
  "rpc failed",
  "http/2 stream",
  "connect to host",
  "early eof",
];

const LOCK_PATTERNS = [
  "index.lock",
  "cannot lock ref",
  "failed to lock",
  "unable to lock",
  "lock exists",
  "another git process",
  ".lock",
];

const DIRTY_PATTERNS = [
  "your local changes",
  "would be overwritten by merge",
  "would be overwritten by checkout",
  "untracked working tree files",
  "please commit your changes or stash",
  "commit or stash",
];

const REJECTED_PATTERNS = [
  "non-fast-forward",
  "fetch first",
  "updates were rejected",
  "! [rejected]",
  "[remote rejected]",
  "failed to push some refs",
  "tip of your current branch is behind",
];

function matchesAny(haystack: string, patterns: string[]): boolean {
  return patterns.some((p) => haystack.includes(p));
}

/**
 * 从错误文本提取分类（纯函数）。匹配顺序与后端一致：认证 → 网络 → 锁 →
 * 脏工作区 → 非快进。返回 null 表示未分类（调用方保持原有展示）。
 */
export function classifyGitErrorText(message: string): GitErrorGuidance | null {
  if (!message) return null;
  const m = message.toLowerCase();

  let category: GitErrorCategoryCode | null = null;
  if (matchesAny(m, AUTH_PATTERNS)) {
    category = "authentication";
  } else if (matchesAny(m, NETWORK_PATTERNS)) {
    category = "network";
  } else if (matchesAny(m, LOCK_PATTERNS)) {
    category = "lock";
  } else if (matchesAny(m, DIRTY_PATTERNS)) {
    category = "dirtyTree";
  } else if (matchesAny(m, REJECTED_PATTERNS)) {
    category = "rejected";
  }
  if (!category) return null;

  return {
    category,
    reason: REASONS[category],
    actions: [...ACTIONS[category]],
  };
}
