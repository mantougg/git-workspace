import { invoke } from "@tauri-apps/api/core";

export interface CrashReportInfo {
  file: string;
  created: string;
  sizeBytes: number;
}

export interface TelemetrySettings {
  enabled: boolean;
  crashUpload: boolean;
}

/** 崩溃报告列表（app data/crash-reports）。 */
export function getCrashReports(): Promise<CrashReportInfo[]> {
  return invoke<CrashReportInfo[]>("get_crash_reports");
}

export function clearCrashReports(): Promise<void> {
  return invoke<void>("clear_crash_reports");
}

/** 一键收集反馈包：logs + crash-reports + note → 目录路径。 */
export function collectFeedbackBundle(note?: string): Promise<string> {
  return invoke<string>("collect_feedback_bundle", { note: note ?? null });
}

/** 遥测配置（opt-in，默认关闭）。 */
export function getTelemetryConfig(): Promise<TelemetrySettings> {
  return invoke<TelemetrySettings>("get_telemetry_config");
}

export function setTelemetryConfig(config: TelemetrySettings): Promise<void> {
  return invoke<void>("set_telemetry_config", { config });
}

/** 记录遥测事件（关闭时后端 no-op）。 */
export function trackEvent(name: string, props?: Record<string, unknown>): Promise<void> {
  return invoke<void>("track_event", { name, props: props ?? null });
}
