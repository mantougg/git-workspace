import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { RepoStatus, RepositoryWithStatus } from "@/types/repository";
import * as repoApi from "@/api/repository";

export const useRepositoryStore = defineStore("repository", () => {
  const repositories = ref<RepositoryWithStatus[]>([]);
  const loading = ref(false);
  const scanning = ref(false);
  const searchQuery = ref("");

  const filteredRepositories = computed(() => {
    if (!searchQuery.value.trim()) return repositories.value;
    const q = searchQuery.value.toLowerCase();
    return repositories.value.filter(
      (r) =>
        r.repository.name.toLowerCase().includes(q) ||
        r.repository.path.toLowerCase().includes(q),
    );
  });

  const totalCount = computed(() => repositories.value.length);
  const cleanCount = computed(
    () =>
      repositories.value.filter((r) => r.status?.isClean).length,
  );

  async function scanRepositories(workspaceId: number) {
    scanning.value = true;
    try {
      repositories.value = await repoApi.scanRepositories(workspaceId);
    } catch (e) {
      console.error("Failed to scan repositories:", e);
      throw e;
    } finally {
      scanning.value = false;
    }
  }

  async function loadRepositories(workspaceId: number) {
    loading.value = true;
    try {
      repositories.value = await repoApi.listRepositories(workspaceId);
    } catch (e) {
      console.error("Failed to load repositories:", e);
    } finally {
      loading.value = false;
    }
  }

  async function scanRepositoriesSelected(
    workspaceId: number,
    subPath: string,
  ) {
    scanning.value = true;
    try {
      repositories.value = await repoApi.scanRepositoriesSelected(
        workspaceId,
        subPath,
      );
    } catch (e) {
      console.error("Failed to scan repository subtree:", e);
      throw e;
    } finally {
      scanning.value = false;
    }
  }

  async function refreshStatus(repoPath: string) {
    try {
      const status = await repoApi.refreshRepositoryStatus(repoPath);
      updateStatus(repoPath, status);
    } catch (e) {
      console.error("Failed to refresh status:", e);
    }
  }

  function updateStatus(repoPath: string, status: RepoStatus) {
    const repo = repositories.value.find(
      (r) => r.repository.path === repoPath,
    );
    if (repo) {
      repo.status = status;
    }
  }

  return {
    repositories,
    loading,
    scanning,
    searchQuery,
    filteredRepositories,
    totalCount,
    cleanCount,
    scanRepositories,
    scanRepositoriesSelected,
    loadRepositories,
    refreshStatus,
    updateStatus,
  };
});
