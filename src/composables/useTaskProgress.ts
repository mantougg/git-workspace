import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";
import { useTaskStore } from "@/stores/task";
import type { TaskProgress } from "@/types/task";
import type { WorkspaceStashProgress } from "@/types/workspaceStash";

export function useTaskProgress() {
  const taskStore = useTaskStore();
  let unlistenFn: (() => void) | null = null;
  let unlistenWsStashFn: (() => void) | null = null;

  onMounted(async () => {
    await taskStore.loadActiveTasks();

    unlistenFn = await listen<TaskProgress>(
      "task_progress",
      (event) => {
        taskStore.updateTaskProgress(event.payload);
      },
    );

    // GF-10：Workspace Stash 逐仓进度（整笔任务体内的细粒度通道）。抽屉关闭
    // 时也要入列——打开面板时才能看到完整明细。
    unlistenWsStashFn = await listen<WorkspaceStashProgress>(
      "workspace_stash_progress",
      (event) => {
        taskStore.applyWorkspaceStashProgress(event.payload);
      },
    );
  });

  onUnmounted(() => {
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
    if (unlistenWsStashFn) {
      unlistenWsStashFn();
      unlistenWsStashFn = null;
    }
  });
}
