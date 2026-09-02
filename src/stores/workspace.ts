import { defineStore } from "pinia";
import { ref } from "vue";
import type {
  CreateWorkspaceRequest,
  UpdateWorkspaceRequest,
  Workspace,
} from "@/types/workspace";
import * as workspaceApi from "@/api/workspace";

const LAST_WORKSPACE_ID_KEY = "gw-last-workspace-id";

export const useWorkspaceStore = defineStore("workspace", () => {
  const workspaces = ref<Workspace[]>([]);
  const currentWorkspace = ref<Workspace | null>(null);
  const loading = ref(false);

  function persistCurrentWorkspace(ws: Workspace | null) {
    if (ws) {
      localStorage.setItem(LAST_WORKSPACE_ID_KEY, String(ws.id));
    } else {
      localStorage.removeItem(LAST_WORKSPACE_ID_KEY);
    }
  }

  async function loadWorkspaces() {
    loading.value = true;
    try {
      workspaces.value = await workspaceApi.listWorkspaces();
      const persistedId = Number(localStorage.getItem(LAST_WORKSPACE_ID_KEY));
      const restored = Number.isInteger(persistedId)
        ? workspaces.value.find((ws) => ws.id === persistedId)
        : undefined;
      const selected = restored ?? workspaces.value[0] ?? null;
      currentWorkspace.value = selected;
      persistCurrentWorkspace(selected);
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
    persistCurrentWorkspace(ws);
    return ws;
  }

  async function removeWorkspace(id: number) {
    await workspaceApi.removeWorkspace(id);
    workspaces.value = workspaces.value.filter((w) => w.id !== id);
    if (currentWorkspace.value?.id === id) {
      const fallback = workspaces.value[0] ?? null;
      currentWorkspace.value = fallback;
      persistCurrentWorkspace(fallback);
    }
  }

  async function updateWorkspace(id: number, req: UpdateWorkspaceRequest) {
    const ws = await workspaceApi.updateWorkspace(id, req);
    const index = workspaces.value.findIndex((w) => w.id === id);
    if (index >= 0) workspaces.value[index] = ws;
    if (currentWorkspace.value?.id === id) {
      currentWorkspace.value = ws;
      persistCurrentWorkspace(ws);
    }
    return ws;
  }

  function selectWorkspace(ws: Workspace) {
    currentWorkspace.value = ws;
    persistCurrentWorkspace(ws);
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
