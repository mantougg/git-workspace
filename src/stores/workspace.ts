import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  Workspace,
} from "@/types/workspace";
import * as workspaceApi from "@/api/workspace";

export const useWorkspaceStore = defineStore("workspace", () => {
  const workspaces = ref<Workspace[]>([]);
  const currentWorkspace = ref<Workspace | null>(null);
  const loading = ref(false);

  async function loadWorkspaces() {
    loading.value = true;
    try {
      workspaces.value = await workspaceApi.listWorkspaces();
      if (workspaces.value.length > 0 && !currentWorkspace.value) {
        currentWorkspace.value = workspaces.value[0];
      }
    } catch (e) {
      console.error("Failed to load workspaces:", e);
    } finally {
      loading.value = false;
    }
  }

  async function addWorkspace(req: CreateWorkspaceRequest) {
    const ws = await workspaceApi.addWorkspace(req);
    workspaces.value.push(ws);
    currentWorkspace.value = ws;
    return ws;
  }

  async function removeWorkspace(id: number) {
    await workspaceApi.removeWorkspace(id);
    workspaces.value = workspaces.value.filter((w) => w.id !== id);
    if (currentWorkspace.value?.id === id) {
      currentWorkspace.value = workspaces.value[0] ?? null;
    }
  }

  async function updateWorkspace(id: number, req: UpdateWorkspaceRequest) {
    const ws = await workspaceApi.updateWorkspace(id, req);
    const index = workspaces.value.findIndex((w) => w.id === id);
    if (index >= 0) workspaces.value[index] = ws;
    if (currentWorkspace.value?.id === id) currentWorkspace.value = ws;
    return ws;
  }

  function selectWorkspace(ws: Workspace) {
    currentWorkspace.value = ws;
  }

  return {
    workspaces,
    currentWorkspace,
    loading,
    loadWorkspaces,
    addWorkspace,
    removeWorkspace,
    updateWorkspace,
    selectWorkspace,
  };
});
