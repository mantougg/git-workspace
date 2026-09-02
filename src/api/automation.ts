import { invoke } from "@tauri-apps/api/core";
import type { Pipeline } from "@/types/pipeline";

// ── 脚本动作（T-32 插件层） ──────────────────────────────────

export interface PluginAction {
  id: number;
  name: string;
  command: string;
  scope: "repo" | "workspace";
  timeoutSecs: number;
  createdAt: string;
  updatedAt: string;
}

export function listPluginActions(): Promise<PluginAction[]> {
  return invoke<PluginAction[]>("list_plugin_actions");
}

export function savePluginAction(action: Partial<PluginAction>): Promise<PluginAction> {
  return invoke<PluginAction>("save_plugin_action", {
    action: {
      id: 0,
      name: "",
      command: "",
      scope: "repo",
      timeoutSecs: 120,
      createdAt: "",
      updatedAt: "",
      ...action,
    },
  });
}

export function deletePluginAction(actionId: number): Promise<void> {
  return invoke<void>("delete_plugin_action", { actionId });
}

/** 运行脚本动作；cwd 按 scope 传仓库根或工作区根。 */
export function runPluginAction(cwd: string, action: PluginAction): Promise<string> {
  return invoke<string>("run_plugin_action", { cwd, action });
}

// ── 定时任务 ────────────────────────────────────────────────

export interface ScheduledTask {
  id: number;
  name: string;
  kind: "script_action" | "pipeline";
  targetId: string;
  scheduleKind: "interval" | "daily";
  intervalMinutes: number | null;
  dailyTime: string | null;
  /** pipeline 的仓库选择 JSON（Vec<RepoSelection>），可空 */
  payload: string | null;
  enabled: boolean;
  lastRun: string | null;
  nextRun: string;
  createdAt: string;
  updatedAt: string;
}

export function listScheduledTasks(): Promise<ScheduledTask[]> {
  return invoke<ScheduledTask[]>("list_scheduled_tasks");
}

export function saveScheduledTask(task: Partial<ScheduledTask>): Promise<ScheduledTask> {
  return invoke<ScheduledTask>("save_scheduled_task", {
    task: {
      id: 0,
      name: "",
      kind: "script_action",
      targetId: "",
      scheduleKind: "interval",
      intervalMinutes: 30,
      dailyTime: null,
      payload: null,
      enabled: true,
      lastRun: null,
      nextRun: "",
      createdAt: "",
      updatedAt: "",
      ...task,
    },
  });
}

export function setScheduledTaskEnabled(taskId: number, enabled: boolean): Promise<void> {
  return invoke<void>("set_scheduled_task_enabled", { taskId, enabled });
}

export function deleteScheduledTask(taskId: number): Promise<void> {
  return invoke<void>("delete_scheduled_task", { taskId });
}

// ── Pipeline 模板导入 / 导出 ─────────────────────────────────

export function exportPipelineTemplate(templateId: string, filePath: string): Promise<string> {
  return invoke<string>("export_pipeline_template", { templateId, filePath });
}

export function importPipelineTemplate(filePath: string): Promise<Pipeline> {
  return invoke<Pipeline>("import_pipeline_template", { filePath });
}
