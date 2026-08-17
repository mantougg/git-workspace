import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  ChangeSet,
  ChangeSetRepoInput,
  ChangeSetSummary,
  CreateChangeSetRequest,
  UpdateChangeSetRequest,
} from "@/types/changeSet";
import * as api from "@/api/changeSet";

/** Change Set state (T-22): list per workspace + the selected set's summary. */
export const useChangeSetStore = defineStore("changeSet", () => {
  const changeSets = ref<ChangeSet[]>([]);
  const currentId = ref<number | null>(null);
  const summary = ref<ChangeSetSummary | null>(null);
  const loading = ref(false);
  const summaryLoading = ref(false);

  async function loadChangeSets(workspaceId: number) {
    loading.value = true;
    try {
      changeSets.value = await api.listChangeSets(workspaceId);
      if (
        currentId.value != null &&
        !changeSets.value.some((c) => c.id === currentId.value)
      ) {
        currentId.value = null;
        summary.value = null;
      }
    } finally {
      loading.value = false;
    }
  }

  async function selectChangeSet(id: number | null) {
    currentId.value = id;
    summary.value = null;
    if (id != null) {
      await refreshSummary();
    }
  }

  async function refreshSummary() {
    if (currentId.value == null) return;
    summaryLoading.value = true;
    try {
      summary.value = await api.getChangeSetSummary(currentId.value);
    } finally {
      summaryLoading.value = false;
    }
  }

  async function createSet(req: CreateChangeSetRequest) {
    const cs = await api.createChangeSet(req);
    changeSets.value.unshift(cs);
    return cs;
  }

  async function updateSet(req: UpdateChangeSetRequest) {
    const cs = await api.updateChangeSet(req);
    const idx = changeSets.value.findIndex((c) => c.id === cs.id);
    if (idx >= 0) changeSets.value[idx] = cs;
    if (summary.value?.changeSet.id === cs.id) {
      summary.value = { ...summary.value, changeSet: cs };
    }
    return cs;
  }

  async function removeSet(id: number) {
    await api.deleteChangeSet(id);
    changeSets.value = changeSets.value.filter((c) => c.id !== id);
    if (currentId.value === id) {
      currentId.value = null;
      summary.value = null;
    }
  }

  async function addRepos(changeSetId: number, repos: ChangeSetRepoInput[]) {
    await api.addChangeSetRepositories(changeSetId, repos);
    if (currentId.value === changeSetId) {
      await refreshSummary();
    }
  }

  async function removeRepo(changeSetId: number, repoId: number) {
    await api.removeChangeSetRepository(changeSetId, repoId);
    if (currentId.value === changeSetId) {
      await refreshSummary();
    }
  }

  return {
    changeSets,
    currentId,
    summary,
    loading,
    summaryLoading,
    loadChangeSets,
    selectChangeSet,
    refreshSummary,
    createSet,
    updateSet,
    removeSet,
    addRepos,
    removeRepo,
  };
});
