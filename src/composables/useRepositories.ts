import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";
import { useRepositoryStore } from "@/stores/repository";
import type { RepoStatus } from "@/types/repository";

export function useRepositories() {
  const repoStore = useRepositoryStore();
  let unlistenFn: (() => void) | null = null;

  onMounted(async () => {
    unlistenFn = await listen<{
      repo_path: string;
      status: RepoStatus;
    }>("repo_status_changed", (event) => {
      const { repo_path, status } = event.payload;
      repoStore.updateStatus(repo_path, status);
    });
  });

  onUnmounted(() => {
    if (unlistenFn) {
      unlistenFn();
      unlistenFn = null;
    }
  });
}
