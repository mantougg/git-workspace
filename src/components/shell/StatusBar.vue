<template>
  <footer class="statusbar">
    <!-- 工作区槽位 -->
    <div class="statusbar-slot statusbar-workspace" @click="showWorkspaceSwitcher">
      <span class="statusbar-dot" />
      <span>{{ currentWorkspaceName }}</span>
      <n-icon :size="10"><ChevronDownOutline /></n-icon>
    </div>

    <div class="statusbar-divider" />

    <!-- 分支槽位（仅 Git 类视图） -->
    <div v-if="currentBranch" class="statusbar-slot">
      <n-icon :size="12"><GitBranchOutline /></n-icon>
      <span>{{ currentBranch }}</span>
    </div>

    <div v-if="currentBranch" class="statusbar-divider" />

    <!-- watcher 槽位 -->
    <div class="statusbar-slot" :title="watcherTooltip">
      <span class="statusbar-dot" :class="watcherActive ? 'dot-active' : 'dot-inactive'" />
    </div>

    <div class="statusbar-divider" />

    <!-- 任务数槽位 -->
    <div
      class="statusbar-slot statusbar-tasks"
      :class="{ clickable: runningTaskCount > 0 }"
      @click="onTasksClick"
    >
      <n-icon :size="12"><PlayOutline /></n-icon>
      <span>{{ runningTaskCount > 0 ? `${runningTaskCount} 个任务` : '无任务' }}</span>
    </div>

    <!-- 弹性占位 -->
    <div class="statusbar-spacer" />

    <!-- 版本槽位 -->
    <div class="statusbar-slot statusbar-version">
      v{{ appVersion }} by {{ appAuthor }}
    </div>
  </footer>

  <!-- 工作区切换器弹层 -->
  <n-popover
    :show="showWsPopover"
    trigger="manual"
    placement="top-start"
    :style="{ marginLeft: '8px' }"
    @clickoutside="showWsPopover = false"
  >
    <template #trigger>
      <div ref="wsTriggerRef" style="position: fixed; bottom: 24px; left: 0; width: 1px; height: 1px;" />
    </template>
    <div class="ws-switcher">
      <div
        v-for="ws in workspaces"
        :key="ws.id"
        class="ws-switcher-item"
        :class="{ active: ws.id === currentWorkspace?.id }"
        @click="switchWorkspace(ws)"
      >
        {{ ws.name }}
      </div>
      <div class="ws-switcher-divider" />
      <div class="ws-switcher-item ws-switcher-manage" @click="goToWorkspaceManage">
        管理工作区…
      </div>
    </div>
  </n-popover>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import { useRouter } from "vue-router";
import { NIcon, NPopover } from "naive-ui";
import { ChevronDownOutline, GitBranchOutline, PlayOutline } from "@vicons/ionicons5";
import { useWorkspaceStore } from "@/stores/workspace";
import { useTaskStore } from "@/stores/task";

// F-07：构建期注入的全局常量
const appVersion = __APP_VERSION__;
const appAuthor = __APP_AUTHOR__;

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const taskStore = useTaskStore();

const showWsPopover = ref(false);
const wsTriggerRef = ref<HTMLElement | null>(null);

// 工作区
const workspaces = computed(() => workspaceStore.workspaces);
const currentWorkspace = computed(() => workspaceStore.currentWorkspace);
const currentWorkspaceName = computed(() => currentWorkspace.value?.name ?? "未选择");

// 分支（仅 Git 类视图显示，此处预留接口，D-04 接入实际数据）
const currentBranch = ref<string | null>(null);

// watcher 状态（预留接口，D-04 接入实际数据）
const watcherActive = ref(false);
const watcherTooltip = computed(() =>
  watcherActive.value ? "监听中" : "未启动"
);

// 任务
const runningTaskCount = computed(() =>
  taskStore.tasks.filter(
    (t) => t.status.type === "queued" || t.status.type === "running"
  ).length
);

function showWorkspaceSwitcher() {
  showWsPopover.value = !showWsPopover.value;
}

function switchWorkspace(ws: { id: number; name: string }) {
  workspaceStore.currentWorkspace = ws as any;
  showWsPopover.value = false;
}

function goToWorkspaceManage() {
  showWsPopover.value = false;
  router.push({ name: "workspaces" });
}

function onTasksClick() {
  if (runningTaskCount.value > 0) {
    taskStore.togglePanel();
  }
}
</script>

<style scoped>
.statusbar {
  height: var(--gw-statusbar-h);
  display: flex;
  align-items: center;
  padding: 0 var(--gw-space-2);
  background: var(--gw-bg-panel);
  border-top: 1px solid var(--gw-border);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  user-select: none;
  flex-shrink: 0;
}

.statusbar-slot {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 var(--gw-space-1);
  height: 100%;
  white-space: nowrap;
}

.statusbar-slot.clickable {
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
}

.statusbar-slot.clickable:hover {
  background: var(--gw-bg-hover);
  color: var(--gw-text);
}

.statusbar-divider {
  width: 1px;
  height: 12px;
  background: var(--gw-border);
  margin: 0 var(--gw-space-1);
}

.statusbar-spacer {
  flex: 1;
}

.statusbar-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
}

.dot-active {
  background: var(--gw-success);
}

.dot-inactive {
  background: var(--gw-text-dim);
}

.statusbar-version {
  margin-left: auto;
}

/* 工作区切换器 */
.ws-switcher {
  min-width: 160px;
}

.ws-switcher-item {
  padding: 6px 12px;
  cursor: pointer;
  font-size: var(--gw-text-sm);
  color: var(--gw-text);
  border-radius: var(--gw-radius-sm);
}

.ws-switcher-item:hover {
  background: var(--gw-bg-hover);
}

.ws-switcher-item.active {
  color: var(--gw-accent);
  font-weight: 500;
}

.ws-switcher-divider {
  height: 1px;
  background: var(--gw-border);
  margin: 4px 0;
}

.ws-switcher-manage {
  color: var(--gw-text-dim);
}
</style>
