import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";
import { useTaskStore } from "@/stores/task";
import type { TaskProgress } from "@/types/task";

export function useTaskProgress() {
  const taskStore = useTaskStore();
  let unlistenFn: (() => void) | null = null;

  onMounted(async () => {
    await taskStore.loadActiveTasks();

    unlistenFn = await listen<TaskProgress>(
      "task_progress",
      (event) => {
        taskStore.updateTaskProgress(event.payload);
      },
    );
  });

  onUnmounted(() => {
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
  });
}
