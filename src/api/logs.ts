import { invoke } from "@tauri-apps/api/core";

export interface LogFileInfo {
  name: string;
  path: string;
  sizeBytes: number;
}

export function listLogFiles(): Promise<LogFileInfo[]> {
  return invoke<LogFileInfo[]>("list_log_files");
}

export function openLogs(): Promise<void> {
  return invoke<void>("open_logs");
}

export function exportLogs(targetDir?: string): Promise<string> {
  return invoke<string>("export_logs", { targetDir: targetDir ?? null });
}

export function clearLogs(): Promise<void> {
  return invoke<void>("clear_logs");
}
