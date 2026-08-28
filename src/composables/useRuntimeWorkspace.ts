// R-13 各视图共用的 workspace 选择 + 事件订阅接线。
// 事件订阅在视图挂载时建立、卸载时释放（幂等），保证状态只在本视图活跃时更新。

import { onMounted, onUnmounted, ref } from "vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRuntimeStore } from "@/stores/runtime";

export function useRuntimeWorkspace() {
  const workspaceStore = useWorkspaceStore();
  const store = useRuntimeStore();
  const ready = ref(false);

  async function ensureWorkspace() {
    // F-15：runtime store 的 workspaceId 派生自全局当前工作区并随 watch
    // 自动加载数据；这里只需保证工作区列表已加载（首个工作区自动选中）。
    await workspaceStore.loadWorkspaces();
    ready.value = true;
  }

  onMounted(async () => {
    // F-15：先确保工作区与数据加载，再订阅事件——订阅失败（如事件名非法）
    // 不得阻断数据展示。
    await ensureWorkspace();
    try {
      await store.subscribe();
    } catch (e) {
      console.error("R-13: runtime event subscribe failed:", e);
    }
  });

  onUnmounted(() => {
    store.unsubscribe();
  });

  return {
    workspaceStore,
    store,
    ready,
    ensureWorkspace,
  };
}
