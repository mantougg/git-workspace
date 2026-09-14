import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** 规则匹配方式（一期：精确名 / 通配符；正则候补）。 */
export type CleanerMatchMode = "exact" | "wildcard";
/**
 * 目标类型。kind 含文件（file / any）即视为该规则显式允许删文件；
 * 目录规则绝不删文件（对齐 cleaner.rs 的 CleanerKind 序列化）。
 */
export type CleanerKind = "dir" | "file" | "any";

export interface CleanerRule {
  pattern: string;
  matchMode: CleanerMatchMode;
  kind: CleanerKind;
}

export interface CleanerScanRequest {
  operationPaths: string[];
  excludePaths: string[];
  rules: CleanerRule[];
}

export interface CleanerItemInfo {
  path: string;
  isDir: boolean;
  /** 文件扫描时直出；目录为 null，由 cleaner_compute_sizes 异步补算。 */
  size: number | null;
}

export interface CleanerScanResult {
  token: string;
  items: CleanerItemInfo[];
  visited: number;
  matched: number;
  warnings: string[];
}

export interface CleanerDeleteItemResult {
  path: string;
  ok: boolean;
  reason?: string;
}

export interface CleanerExecuteResult {
  deleted: number;
  failed: number;
  items: CleanerDeleteItemResult[];
}

export interface CleanerScanProgress {
  token: string;
  visited: number;
  matched: number;
  done: boolean;
}

export interface CleanerSizeProgress {
  token: string;
  /** done 帧的 path 为空串。 */
  path: string;
  size: number;
  done: boolean;
}

/** 扫描预览（只读）。结果保存在服务端会话，删除只认返回的 token。 */
export function cleanerScan(req: CleanerScanRequest) {
  return invoke<CleanerScanResult>("cleaner_scan", { req });
}

/** 后台补算目录大小，进度经 cleaner_size_progress 事件推送。 */
export function cleanerComputeSizes(token: string) {
  return invoke<void>("cleaner_compute_sizes", { token });
}

/**
 * 危险操作：永久删除（不进回收站）。
 * 调用前必须经用户输入 DELETE 确认；后端强制 confirmed=true、
 * 路径必须来自 token 对应的扫描会话，执行后会话失效（需重新扫描）。
 */
export function cleanerExecute(token: string, paths: string[]) {
  return invoke<CleanerExecuteResult>("cleaner_execute", {
    token,
    paths,
    confirmed: true,
  });
}

export function onCleanerScanProgress(
  cb: (p: CleanerScanProgress) => void,
): Promise<UnlistenFn> {
  return listen<CleanerScanProgress>("cleaner_scan_progress", (e) =>
    cb(e.payload),
  );
}

export function onCleanerSizeProgress(
  cb: (p: CleanerSizeProgress) => void,
): Promise<UnlistenFn> {
  return listen<CleanerSizeProgress>("cleaner_size_progress", (e) =>
    cb(e.payload),
  );
}
