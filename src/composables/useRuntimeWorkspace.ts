// R-13 各视图共用的 workspace 选择 + 事件订阅接线。
// 事件订阅在视图挂载时建立、卸载时释放（幂等），保证状态只在本视图活跃时更新。

import { onMounted, onUnmounted, ref } from "vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRuntimeStore } from "@/stores/runtime";

export function useRuntimeWorkspace() {
  const workspaceStore = useWorkspaceStore();
  const store = useRuntimeStore();
  const selectedWorkspaceId = ref<number | null>(store.workspaceId);
  const ready = ref(false);

  async function ensureWorkspace() {
    await workspaceStore.loadWorkspaces();
    let id = store.workspaceId;
    if (id == null) {
      id =
        workspaceStore.currentWorkspace?.id ??
        workspaceStore.workspaces[0]?.id ??
        null;
    }
    if (id != null) {
      selectedWorkspaceId.value = id;
      if (id !== store.workspaceId) {
        await store.setWorkspace(id);
      }
    }
    ready.value = true;
  }

  async function selectWorkspace(id: number) {
    selectedWorkspaceId.value = id;
    const ws = workspaceStore.workspaces.find((w) => w.id === id);
    if (ws) workspaceStore.selectWorkspace(ws);
    await store.setWorkspace(id);
  }

  onMounted(async () => {
    await store.subscribe();
    await ensureWorkspace();
  });

  onUnmounted(() => {
    store.unsubscribe();
  });

  return { workspaceStore, store, selectedWorkspaceId, ready, selectWorkspace };
}
