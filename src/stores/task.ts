import { defineStore } from "pinia";
import { ref } from "vue";
import type { Task, TaskEventEntry, TaskProgress } from "@/types/task";
import { TASK_EVENT_LOG_MAX } from "@/types/task";
import * as taskApi from "@/api/task";

export const useTaskStore = defineStore("task", () => {
  /** 当前状态索引：供计数 / 取消 / waitForTasks / clearFinished 使用。 */
  const tasks = ref<Task[]>([]);
  /**
   * TM-08：append-only 命令流事件日志（面板按时间序渲染）。刷新后仅含本次
   * 会话事件——`loadActiveTasks` 只恢复活跃任务，不恢复历史，可接受。
   */
  const events = ref<TaskEventEntry[]>([]);
  const panelVisible = ref(false);

  /** 事件序列号（单调递增；同一条 progress 重放不会产生新 seq）。 */
  let eventSeq = 0;
  /** 每个任务的首个事件 seq（queued / running 边界去重：同态不连续追加）。 */
  const lastEventSeqByTask = new Map<string, { seq: number; type: string }>();

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

  /**
   * TM-08：记录一条命令流事件。同任务同状态类型的连续事件合流（后端
   * `task_progress` 无 seq，同态重放/高频刷新只保留边界），queued→running
   * 的进度百分比刷新不逐条入列。返回是否真正入列。
   */
  function appendEvent(
    progress: TaskProgress,
    at: number,
    durationMs?: number
  ): boolean {
    const prev = lastEventSeqByTask.get(progress.taskId);
    if (prev && prev.type === progress.status.type) {
      // 同态：仅 running 的进度刷新与终态后的重放；终态已在列，直接跳过。
      return false;
    }
    // queued：TTL 早退的 cancelled 是无前序事件的首态，也入列。
    eventSeq += 1;
    const entry: TaskEventEntry = {
      seq: eventSeq,
      taskId: progress.taskId,
      at,
      repoPath: progress.repoPath,
      repoName: progress.repoName,
      taskType: progress.taskType,
      status: progress.status,
      durationMs,
      batchId: progress.batchId ?? null,
    };
    events.value.push(entry);
    if (events.value.length > TASK_EVENT_LOG_MAX) {
      events.value.splice(0, events.value.length - TASK_EVENT_LOG_MAX);
    }
    lastEventSeqByTask.set(progress.taskId, {
      seq: eventSeq,
      type: progress.status.type,
    });
    return true;
  }

  function updateTaskProgress(progress: TaskProgress) {
    const idx = tasks.value.findIndex((t) => t.id === progress.taskId);
    const prev = idx >= 0 ? tasks.value[idx] : undefined;
    const updatedTask: Task = {
      id: progress.taskId,
      taskType: progress.taskType,
      repoPath: progress.repoPath,
      repoName: progress.repoName,
      status: progress.status,
      createdAt: prev?.createdAt ?? new Date().toISOString(),
      // Batch grouping key (T-20): children carry the batch id; the batch
      // row itself arrives with batchId null and id == batch id.
      batchId: progress.batchId ?? null,
    };

    if (idx >= 0) {
      tasks.value[idx] = updatedTask;
    } else {
      tasks.value.unshift(updatedTask);
    }

    const now = Date.now();
    const durationMs =
      prev && isTerminal(progress.status)
        ? Math.max(0, now - Date.parse(prev.createdAt))
        : undefined;
    const appended = appendEvent(progress, now, durationMs);

    // TM-08：任务启动不再自动弹面板；仅失败 / 取消时自动展开命令流。
    if (
      appended &&
      (progress.status.type === "failed" ||
        progress.status.type === "cancelled")
    ) {
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
      // 终态事件随任务一起撤离时间线；进行中事件保留。
      events.value = events.value.filter((e) => {
        const t = tasks.value.find((x) => x.id === e.taskId);
        // 任务仍活跃（含 batch 行——children 仍在列时父行保留）→ 保留事件；
        // 已无对应活跃任务 → 事件过期。
        return !!t;
      });
      const liveIds = new Set(tasks.value.map((t) => t.id));
      for (const id of [...lastEventSeqByTask.keys()]) {
        if (!liveIds.has(id)) lastEventSeqByTask.delete(id);
      }
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
    events,
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
