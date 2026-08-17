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
    updateTaskProgress,
    cancelTask,
    clearFinished,
    togglePanel,
    showPanel,
    hidePanel,
  };
});
