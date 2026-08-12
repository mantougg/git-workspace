import { invoke } from "@tauri-apps/api/core";
import type { Task, TaskRequest } from "@/types/task";

export function submitTasks(tasks: TaskRequest[]): Promise<string[]> {
  return invoke<string[]>("submit_tasks", { tasks });
}

export function getTaskStatus(taskIds: string[]): Promise<Task[]> {
  return invoke<Task[]>("get_task_status", { taskIds });
}

export function cancelTask(taskId: string): Promise<void> {
  return invoke<void>("cancel_task", { taskId });
}

export function listActiveTasks(): Promise<Task[]> {
  return invoke<Task[]>("list_active_tasks");
}

export function clearFinishedTasks(): Promise<void> {
  return invoke<void>("clear_finished_tasks");
}
