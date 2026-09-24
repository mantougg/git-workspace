<template>
  <footer class="statusbar">
    <!-- 工作区槽位 -->
    <div class="statusbar-slot statusbar-workspace" @click="showWorkspaceSwitcher">
      <span class="statusbar-dot" />
      <span>{{ currentWorkspaceName }}</span>
      <n-icon :size="10"><ChevronDownOutline /></n-icon>
    </div>

    <div class="statusbar-divider" />

    <!-- 分支槽位（仅 Git 类视图，点击复制分支名；无上下文时隐藏，desktop-skin-plan:211） -->
    <div
      v-if="currentBranch"
      class="statusbar-slot clickable"
      title="当前分支（点击复制）"
      @click="copyCurrentBranch"
    >
      <n-icon :size="12"><GitBranchOutline /></n-icon>
      <span>{{ currentBranch }}</span>
    </div>

    <div v-if="currentBranch" class="statusbar-divider" />

    <!-- watcher 槽位 -->
    <div class="statusbar-slot" :title="watcherTooltip">
      <span class="statusbar-dot" :class="watcherActive ? 'dot-active' : 'dot-inactive'" />
    </div>

    <div class="statusbar-divider" />

    <!-- 任务数槽位：TM-08 增加失败红色标识 -->
    <div
      class="statusbar-slot statusbar-tasks"
      :class="{
        clickable: runningTaskCount > 0 || failedTaskCount > 0,
        'task-failed': failedTaskCount > 0,
      }"
      :title="failedTaskCount > 0 ? `${failedTaskCount} 个任务失败` : undefined"
      @click="onTasksClick"
    >
      <n-icon :size="12">
        <AlertCircleOutline v-if="failedTaskCount > 0" />
        <PlayOutline v-else />
      </n-icon>
      <span v-if="runningTaskCount > 0">{{ runningTaskCount }} 个任务</span>
      <span v-else-if="failedTaskCount > 0">{{ failedTaskCount }} 个失败</span>
      <span v-else>无任务</span>
    </div>

    <!-- 内嵌终端槽位（TM-02，StatusBar 全局唯一开合入口） -->
    <div class="statusbar-slot clickable" title="终端（Ctrl+`）" @click="terminalStore.togglePanel()">
      <n-icon :size="12"><TerminalOutline /></n-icon>
      <span>终端</span>
    </div>

    <!-- 弹性占位 -->
    <div class="statusbar-spacer" />

    <!-- 在编辑器中打开槽位 -->
    <div
      v-if="availableIdes.length > 0"
      class="statusbar-slot clickable"
      title="在编辑器中打开"
      @click="toggleIdePopover"
    >
      <n-icon :size="12"><CodeOutline /></n-icon>
      <span>打开编辑器</span>
      <n-icon :size="10"><ChevronDownOutline /></n-icon>
    </div>

    <div v-if="availableIdes.length > 0" class="statusbar-divider" />

    <!-- AI 助手槽位（AI-10：Drawer 全局唯一入口之一，快捷键 Ctrl+I） -->
    <div class="statusbar-slot clickable" title="AI 助手（Ctrl+I）" @click="aiStore.toggleDrawer()">
      <n-icon :size="12"><SparklesOutline /></n-icon>
      <span>AI 助手</span>
    </div>

    <div class="statusbar-divider" />

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

  <!-- 编辑器选择器弹层 -->
  <n-popover
    :show="showIdePopover"
    trigger="manual"
    placement="top-end"
    :style="{ marginRight: '8px' }"
    @clickoutside="showIdePopover = false"
  >
    <template #trigger>
      <div ref="ideTriggerRef" style="position: fixed; bottom: 24px; right: 0; width: 1px; height: 1px;" />
    </template>
    <div class="ide-switcher">
      <div
        v-for="ide in availableIdes"
        :key="ide"
        class="ide-switcher-item"
        @click="openInEditor(ide)"
      >
        {{ ideDisplayName(ide) }}
      </div>
      <div class="ide-switcher-divider" />
      <div class="ide-switcher-item" @click="openFileManager">
        <n-icon :size="12"><FolderOpenOutline /></n-icon>
        <span>打开文件管理器</span>
      </div>
      <div class="ide-switcher-item" @click="openWithSystemChooser">
        更多…
      </div>
    </div>
  </n-popover>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { NIcon, NPopover, useMessage } from "naive-ui";
import { ChevronDownOutline, CodeOutline, FolderOpenOutline, GitBranchOutline, PlayOutline, SparklesOutline, TerminalOutline, AlertCircleOutline } from "@vicons/ionicons5";
import { WATCHER_EVENTS, watcherStatus } from "@/api/git_ops";
import { listIntegrationTargets, openInIde, openInFileManager, openWithSystemApp, IDE_DISPLAY_NAMES, type IdeKind } from "@/api/integration";
import { refreshRepositoryStatus } from "@/api/repository";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskStore } from "@/stores/task";
import { useAiStore } from "@/stores/ai";
import { useTerminalStore } from "@/stores/terminal";
import type { Workspace } from "@/types/workspace";
import type { RepoStatusUpdate } from "@/types/events";

// F-07：构建期注入的全局常量
const appVersion = __APP_VERSION__;
const appAuthor = __APP_AUTHOR__;

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();
const taskStore = useTaskStore();
const aiStore = useAiStore();
const terminalStore = useTerminalStore();
const message = useMessage();

const showWsPopover = ref(false);
const wsTriggerRef = ref<HTMLElement | null>(null);

// 编辑器
const showIdePopover = ref(false);
const ideTriggerRef = ref<HTMLElement | null>(null);
const availableIdes = ref<IdeKind[]>([]);

function ideDisplayName(ide: IdeKind): string {
  return IDE_DISPLAY_NAMES[ide] ?? ide;
}

function toggleIdePopover() {
  showIdePopover.value = !showIdePopover.value;
}

async function openInEditor(ide: IdeKind) {
  const path =
    currentWorkspace.value?.path ||
    "";
  if (!path) return;
  showIdePopover.value = false;
  try {
    await openInIde(path, ide);
  } catch (e) {
    console.error("Failed to open IDE:", e);
  }
}

async function openFileManager() {
  const path = currentWorkspace.value?.path || "";
  if (!path) return;
  showIdePopover.value = false;
  try {
    await openInFileManager(path);
  } catch (e) {
    console.error("Failed to open file manager:", e);
  }
}

async function openWithSystemChooser() {
  const path = currentWorkspace.value?.path || "";
  if (!path) return;
  showIdePopover.value = false;
  try {
    await openWithSystemApp(path);
  } catch (e) {
    console.error("Failed to open system app chooser:", e);
  }
}

// 工作区
const workspaces = computed(() => workspaceStore.workspaces);
const currentWorkspace = computed(() => workspaceStore.currentWorkspace);
const currentWorkspaceName = computed(() => currentWorkspace.value?.name ?? "未选择");

// GF-19：分支槽位（原为恒 null 的空占位，D-04 预留未接）。
// 数据源：repoStore——当前仓库（F-14 全局态）+ repositories 的 status.branch
// （watcher 的 repo_status_changed_batch 经 useRepositories/updateStatus 持续刷新）。
// 仅 Git 类视图显示（meta.group === "Git" + 三个 repo 上下文任务页）；
// 无当前仓库上下文时槽位隐藏（desktop-skin-plan:211）。
const GIT_VIEW_ROUTE_NAMES = new Set(["changes", "diff-viewer", "conflict-resolver"]);
const isGitView = computed(() => {
  const route = router.currentRoute.value;
  return route.meta.group === "Git" || GIT_VIEW_ROUTE_NAMES.has(String(route.name));
});

// 路径匹配两侧归一化分隔符（Windows 混合分隔符，见 AGENTS.md 平台规范）。
const normPath = (p: string) => p.replace(/\\/g, "/");

const currentRepoEntry = computed(() => {
  if (!isGitView.value) return null;
  const path = repoStore.currentRepoPath;
  if (!path) return null;
  // 列表属于当前工作区才可信——否则 currentRepoPath 可能是切换工作区前的旧仓库。
  const wsId = workspaceStore.currentWorkspace?.id;
  if (wsId != null && repoStore.repositoriesWorkspaceId !== wsId) return null;
  return (
    repoStore.repositories.find((r) => normPath(r.repository.path) === normPath(path)) ?? null
  );
});

// 当前仓库不在已加载列表内时（如 SideNav 直达 Git 视图、列表尚未加载）的
// 一次性兜底查询；repoStore.repositories 命中时不发请求。
const fallbackBranch = ref<string | null>(null);
watch(
  [currentRepoEntry, () => repoStore.currentRepoPath, isGitView],
  async ([entry, path, gitView]) => {
    fallbackBranch.value = null;
    if (!gitView || !path || entry?.status) return;
    try {
      const status = await refreshRepositoryStatus(path);
      fallbackBranch.value = status.isDetached ? "HEAD 游离" : status.branch || null;
    } catch {
      // 查询失败保持隐藏（不显示错误 toast：StatusBar 是常驻组件）
    }
  },
  { immediate: true },
);

const currentBranch = computed(() => {
  const status = currentRepoEntry.value?.status ?? null;
  if (status) {
    if (status.isDetached) return "HEAD 游离";
    return status.branch || null;
  }
  return fallbackBranch.value;
});

async function copyCurrentBranch() {
  if (!currentBranch.value) return;
  try {
    await navigator.clipboard.writeText(currentBranch.value);
    message.success(`已复制分支名：${currentBranch.value}`);
  } catch (e) {
    console.error("Failed to copy branch name:", e);
  }
}

// watcher 状态：从后端查询初始值，并通过启停事件保持同步。
const watcherActive = ref(false);
const watcherTooltip = computed(() =>
  watcherActive.value ? "监听中" : "未启动"
);
let unlistenWatcher: UnlistenFn | null = null;
// GF-19：repo_status_changed_batch 监听（分支槽位数据保鲜）。
let unlistenRepoStatus: UnlistenFn | null = null;

async function loadWatcherStatus() {
  try {
    watcherActive.value = await watcherStatus();
  } catch (e) {
    console.error("Failed to load watcher status:", e);
  }
}

// 任务
const runningTaskCount = computed(() =>
  taskStore.tasks.filter(
    (t) => t.status.type === "queued" || t.status.type === "running"
  ).length
);
// TM-08：失败任务计数（命令流红色标识）
const failedTaskCount = computed(() =>
  taskStore.tasks.filter((t) => t.status.type === "failed").length
);

function showWorkspaceSwitcher() {
  showWsPopover.value = !showWsPopover.value;
}

function switchWorkspace(ws: Workspace) {
  workspaceStore.selectWorkspace(ws);
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

onMounted(async () => {
  unlistenWatcher = await listen<boolean>(WATCHER_EVENTS.statusChanged, (event) => {
    watcherActive.value = event.payload;
  });
  // GF-19：分支槽位的数据保鲜。watcher 的 repo_status_changed_batch 只在
  // 变更页/总览页有监听方；Git 类视图（分支切换等本地操作后）需要 StatusBar
  // 自己消费，否则 store 里的 status.branch 会陈旧。
  unlistenRepoStatus = await listen<RepoStatusUpdate[]>(
    "repo_status_changed_batch",
    (event) => {
      for (const { repoPath, status } of event.payload) {
        repoStore.updateStatus(repoPath, status);
      }
    },
  );
  await loadWatcherStatus();

  // 加载可用编辑器列表
  try {
    const targets = await listIntegrationTargets();
    availableIdes.value = targets.ides as IdeKind[];
  } catch (e) {
    console.error("Failed to load integration targets:", e);
  }
});

onUnmounted(() => {
  unlistenWatcher?.();
  unlistenWatcher = null;
  unlistenRepoStatus?.();
  unlistenRepoStatus = null;
});
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

/* TM-08：任务槽失败红色标识 */
.statusbar-tasks.task-failed {
  color: var(--gw-danger);
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

/* 编辑器选择器 */
.ide-switcher {
  min-width: 160px;
}

.ide-switcher-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  cursor: pointer;
  font-size: var(--gw-text-sm);
  color: var(--gw-text);
  border-radius: var(--gw-radius-sm);
}

.ide-switcher-item:hover {
  background: var(--gw-bg-hover);
}

.ide-switcher-divider {
  height: 1px;
  background: var(--gw-border);
  margin: 4px 0;
}
</style>
