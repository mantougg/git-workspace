/**
 * Terminal API — Tauri invoke 封装（TM-01/TM-02，terminal-feature-plan §4.2 IPC 契约）。
 *
 * 6 个 command + 2 个事件，名称/payload 与方案 §4.2 完全一致。
 */

import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Types（与后端 pty.rs 的 serde 结构一一对应）
// ---------------------------------------------------------------------------

export interface TerminalOpenParams {
  cwd?: string;
  shell?: string;
  cols: number;
  rows: number;
}

export interface TerminalWriteParams {
  sessionId: string;
  dataBase64: string;
}

export interface TerminalResizeParams {
  sessionId: string;
  cols: number;
  rows: number;
}

export interface TerminalCloseParams {
  sessionId: string;
}

export interface TerminalSessionInfo {
  sessionId: string;
  kind: "shell" | "runtime";
  title: string;
  cwd: string;
  alive: boolean;
}

export interface ShellInfo {
  id: string;
  label: string;
  path: string;
}

export interface TerminalOutputEvent {
  sessionId: string;
  dataBase64: string;
}

export interface TerminalExitEvent {
  sessionId: string;
  exitCode: number | null;
}

export interface GitOpOutputEvent {
  repoPath: string;
  repoName: string;
  command: string;
  stream: "stdout" | "stderr" | "meta";
  line: string;
}

// ---------------------------------------------------------------------------
// Event names（常量，snake_case，无 `.`）
// ---------------------------------------------------------------------------

export const TERMINAL_EVENTS = {
  OUTPUT: "terminal_output",
  EXIT: "terminal_exit",
  GIT_OP_OUTPUT: "git_op_output",
} as const;

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/** 打开新 PTY 会话（shell 探测按 §5.1 顺序，cwd 缺省当前工作区根）。 */
export function terminalOpen(params: TerminalOpenParams): Promise<string> {
  return invoke<string>("terminal_open", { params });
}

/** 向 PTY 写入（base64 编码的字节流）。 */
export function terminalWrite(params: TerminalWriteParams): Promise<void> {
  return invoke<void>("terminal_write", { params });
}

/** 缩放 PTY（xterm fit 时同步 cols/rows）。 */
export function terminalResize(params: TerminalResizeParams): Promise<void> {
  return invoke<void>("terminal_resize", { params });
}

/** 关闭会话（优雅 → 强杀，复用 kill_tree.rs）。 */
export function terminalClose(params: TerminalCloseParams): Promise<void> {
  return invoke<void>("terminal_close", { params });
}

/** 列出存活会话（面板重开时恢复）。 */
export function terminalList(): Promise<TerminalSessionInfo[]> {
  return invoke<TerminalSessionInfo[]>("terminal_list");
}

/** 列出可用 shell（TM-07 新建 tab 下拉）。 */
export function terminalListShells(): Promise<ShellInfo[]> {
  return invoke<ShellInfo[]>("terminal_list_shells");
}

/** TM-06：在终端中启动 runtime（降级模式）。 */
export function runtimeStartInTerminal(
  command: string,
  cwd?: string,
  env?: Record<string, string>
): Promise<string> {
  return invoke<string>("runtime_start_in_terminal", { command, cwd, env });
}
