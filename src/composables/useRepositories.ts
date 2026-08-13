import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";
import { useRepositoryStore } from "@/stores/repository";
import type { RepoStatus } from "@/types/repository";

export function useRepositories() {
  const repoStore = useRepositoryStore();
  let unlistenFn: (() => void) | null = null;

  onMounted(async () => {
    unlistenFn = await listen<Array<{ repoPath: string; status: RepoStatus }>>(
      "repo_status_changed_batch",
      (event) => {
        for (const { repoPath, status } of event.payload) {
          repoStore.updateStatus(repoPath, status);
        }
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
