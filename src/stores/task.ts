import { defineStore } from "pinia";
import { ref } from "vue";
import type { Task, TaskProgress } from "@/types/task";
import * as taskApi from "@/api/task";

export const useTaskStore = defineStore("task", () => {
  const tasks = ref<Task[]>([]);
  const panelVisible = ref(false);

  async function loadActiveTasks() {
    try {
      tasks.value = await taskApi.listActiveTasks();
    } catch (e) {
      console.error("Failed to load active tasks:", e);
    }
  }

  function isTerminal(status: Task["status"]): boolean {
    return ["success", "partialSuccess", "failed", "cancelled"].includes(
      status.type
    );
  }

  /**
   * PAF-11：等待提交的任务到达终态（轮询活跃列表；任务不再活跃即视为
   * 已收口）。供 batch_add/batch_restore 等队列化命令在刷新视图前等待。
   * 超时保护避免 UI 卡死——超时后由调用方决定是否照常刷新。
   */
  async function waitForTasks(taskIds: string[], timeoutMs = 60_000) {
    if (taskIds.length === 0) return;
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      await loadActiveTasks();
      const pending = taskIds.filter((id) => {
        const t = tasks.value.find((x) => x.id === id);
        return t ? !isTerminal(t.status) : false;
      });
      if (pending.length === 0) return;
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    console.warn("waitForTasks timed out; refreshing anyway", taskIds);
  }

  function updateTaskProgress(progress: TaskProgress) {
    const idx = tasks.value.findIndex((t) => t.id === progress.taskId);
    const updatedTask: Task = {
      id: progress.taskId,
      taskType: progress.taskType,
      repoPath: progress.repoPath,
      repoName: progress.repoName,
      status: progress.status,
      createdAt: new Date().toISOString(),
      // Batch grouping key (T-20): children carry the batch id; the batch
      // row itself arrives with batchId null and id == batch id.
      batchId: progress.batchId ?? null,
    };

    if (idx >= 0) {
      tasks.value[idx] = updatedTask;
    } else {
      tasks.value.unshift(updatedTask);
    }

    // Auto-show panel when tasks are running
    const hasRunning = tasks.value.some(
      (t) => t.status.type === "queued" || t.status.type === "running",
    );
    if (hasRunning) {
      panelVisible.value = true;
    }
  }

  async function cancelTask(taskId: string) {
    try {
      await taskApi.cancelTask(taskId);
      const idx = tasks.value.findIndex((t) => t.id === taskId);
      if (idx >= 0) {
        tasks.value[idx] = {
          ...tasks.value[idx],
          status: { type: "cancelled" },
        };
      }
    } catch (e) {
      console.error("Failed to cancel task:", e);
    }
  }

  async function clearFinished() {
    try {
      await taskApi.clearFinishedTasks();
      tasks.value = tasks.value.filter(
        (t) => t.status.type === "queued" || t.status.type === "running",
      );
    } catch (e) {
      console.error("Failed to clear finished tasks:", e);
    }
  }

  function togglePanel() {
    panelVisible.value = !panelVisible.value;
  }

  function showPanel() {
    panelVisible.value = true;
  }

  function hidePanel() {
    panelVisible.value = false;
  }

  return {
    tasks,
    panelVisible,
    loadActiveTasks,
    waitForTasks,
    updateTaskProgress,
    cancelTask,
    clearFinished,
    togglePanel,
    showPanel,
    hidePanel,
  };
});
