<!--
  TerminalPanel — 底部 drawer + tab 容器（TM-02，terminal-feature-plan §4.3）。
  仿 TaskPanel.vue：底部、可拖高 240px~85vh，StatusBar 开合。
  挂载进 App.vue 全局 overlay 区。
-->

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from "vue";
import { useTerminalStore } from "@/stores/terminal";
import TerminalTabs from "./TerminalTabs.vue";
import XtermView from "./XtermView.vue";

const terminalStore = useTerminalStore();
const activeSession = computed(() => terminalStore.activeSession);

// TM-05：Runtime tab 判断
const isRuntimeTab = computed(() =>
  activeTabId.value?.startsWith("__runtime_") ?? false
);
const isGitConsoleTab = computed(() =>
  activeTabId.value === "__git_console__"
);
const currentRuntimeName = computed(() => {
  if (!activeTabId.value?.startsWith("__runtime_")) return null;
  return activeTabId.value.replace("__runtime_", "");
});
const isRuntimeRunningState = computed(() => {
  const name = currentRuntimeName.value;
  return name ? terminalStore.isRuntimeRunning(name) : false;
});
const isRuntimeBuildingState = computed(() => {
  const name = currentRuntimeName.value;
  return name ? terminalStore.isRuntimeBuilding(name) : false;
});

async function startCurrentRuntime() {
  const name = currentRuntimeName.value;
  if (name) await terminalStore.startRuntime(name);
}
async function stopCurrentRuntime() {
  const name = currentRuntimeName.value;
  if (name) await terminalStore.stopRuntime(name);
}
async function restartCurrentRuntime() {
  const name = currentRuntimeName.value;
  if (name) await terminalStore.restartRuntime(name);
}

// ---------------------------------------------------------------------------
// TM-07：搜索
// ---------------------------------------------------------------------------
const showSearch = ref(false);
const searchText = ref("");
const searchCaseSensitive = ref(false);

function toggleSearch() {
  showSearch.value = !showSearch.value;
  if (showSearch.value) {
    // 聚焦搜索框
    setTimeout(() => {
      const input = document.querySelector(".terminal-search-input input") as HTMLInputElement;
      input?.focus();
    }, 50);
  }
}

function doSearchNext() {
  if (!searchText.value || !activeTabId.value) return;
  const xtermRef = xtermRefs.value.get(activeTabId.value);
  xtermRef?.findNext(searchText.value, { caseSensitive: searchCaseSensitive.value });
}

function doSearchPrevious() {
  if (!searchText.value || !activeTabId.value) return;
  const xtermRef = xtermRefs.value.get(activeTabId.value);
  xtermRef?.findPrevious(searchText.value, { caseSensitive: searchCaseSensitive.value });
}

// ---------------------------------------------------------------------------
// TM-07：清屏 / 重开
// ---------------------------------------------------------------------------
function clearScreen() {
  if (!activeTabId.value) return;
  const xtermRef = xtermRefs.value.get(activeTabId.value);
  xtermRef?.clear();
}

async function reopenSession() {
  const session = terminalStore.activeSession;
  if (!session || session.alive) return;
  // 关闭旧 tab，用同 cwd/shell 重新打开
  await terminalStore.closeTab(session.sessionId);
  await terminalStore.openSession({ cwd: session.cwd });
}

// ---------------------------------------------------------------------------
// TM-07：面板最大化
// ---------------------------------------------------------------------------
const isMaximized = ref(false);
const savedHeight = ref(320);

function toggleMaximize() {
  if (isMaximized.value) {
    panelHeight.value = savedHeight.value;
    isMaximized.value = false;
  } else {
    savedHeight.value = panelHeight.value;
    panelHeight.value = window.innerHeight - 24; // 减去 StatusBar 高度
    isMaximized.value = true;
  }
}

// ---------------------------------------------------------------------------
// Panel visibility & height
// ---------------------------------------------------------------------------

const visible = computed({
  get: () => terminalStore.panelVisible,
  set: (v) => {
    if (v) terminalStore.showPanel();
    else terminalStore.hidePanel();
  },
});

const panelHeight = ref(320);
const MIN_HEIGHT = 240;
const MAX_HEIGHT_RATIO = 0.85;

// ---------------------------------------------------------------------------
// Drag resize（仿 TaskPanel.vue:232-256）
// ---------------------------------------------------------------------------

let startY = 0;
let startHeight = 0;

function startResize(e: MouseEvent) {
  startY = e.clientY;
  startHeight = panelHeight.value;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
  document.body.style.userSelect = "none";
  document.body.style.cursor = "ns-resize";
}

function onResizeMove(e: MouseEvent) {
  const delta = startY - e.clientY;
  const maxHeight = window.innerHeight * MAX_HEIGHT_RATIO;
  panelHeight.value = Math.max(MIN_HEIGHT, Math.min(maxHeight, startHeight + delta));
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
  document.body.style.userSelect = "";
  document.body.style.cursor = "";
}

onBeforeUnmount(() => {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
});

// ---------------------------------------------------------------------------
// Tab management
// ---------------------------------------------------------------------------

const activeTabId = computed(() => terminalStore.activeTabId);
const sessions = computed(() => terminalStore.sessions);

/** 面板首次打开时刷新会话列表。 */
watch(visible, (v) => {
  if (v) {
    terminalStore.refreshSessions();
  }
});

// ---------------------------------------------------------------------------
// Xterm interaction
// ---------------------------------------------------------------------------

/** xterm 实例引用（按 sessionId 索引）。 */
const xtermRefs = ref<Map<string, InstanceType<typeof XtermView>>>(new Map());

function onXtermInput(sessionId: string, dataBase64: string) {
  terminalStore.writeToSession(sessionId, dataBase64);
}

function onXtermResize(sessionId: string, cols: number, rows: number) {
  terminalStore.resizeSession(sessionId, cols, rows);
}

function registerXtermRef(sessionId: string, ref: InstanceType<typeof XtermView> | null) {
  if (ref) {
    xtermRefs.value.set(sessionId, ref);
  } else {
    xtermRefs.value.delete(sessionId);
  }
}

// ---------------------------------------------------------------------------
// 初始加载
// ---------------------------------------------------------------------------

onMounted(() => {
  terminalStore.loadShells();
});
</script>

<template>
  <div v-if="visible" class="terminal-panel" :style="{ height: panelHeight + 'px' }">
    <!-- 拖高条 -->
    <div class="terminal-panel-resize-handle" @mousedown="startResize" />

    <!-- Tab 条 + 工具条 -->
    <div class="terminal-panel-header">
      <TerminalTabs />
      <div class="terminal-panel-toolbar">
        <!-- TM-05：Runtime 控制按钮（仅 runtime tab 显示） -->
        <template v-if="isRuntimeTab">
          <button
            class="terminal-toolbar-btn runtime-btn"
            title="启动"
            :disabled="isRuntimeBuildingState || isRuntimeRunningState"
            @click="startCurrentRuntime"
          >
            ▶
          </button>
          <button
            class="terminal-toolbar-btn runtime-btn"
            title="重启"
            :disabled="isRuntimeBuildingState || !isRuntimeRunningState"
            @click="restartCurrentRuntime"
          >
            🔄
          </button>
          <button
            class="terminal-toolbar-btn runtime-btn"
            title="停止"
            :disabled="isRuntimeBuildingState || !isRuntimeRunningState"
            @click="stopCurrentRuntime"
          >
            ⏹
          </button>
          <div class="terminal-toolbar-divider" />
        </template>
        <!-- 搜索按钮 -->
        <button class="terminal-toolbar-btn" title="搜索（Ctrl+F）" @click="toggleSearch">🔍</button>
        <!-- 清屏按钮 -->
        <button class="terminal-toolbar-btn" title="清屏" @click="clearScreen">🗑</button>
        <!-- 重开按钮（仅已退出会话显示） -->
        <button
          v-if="activeSession && !activeSession.alive && !isRuntimeTab && !isGitConsoleTab"
          class="terminal-toolbar-btn"
          title="重开会话"
          @click="reopenSession"
        >
          🔄
        </button>
        <!-- 最大化按钮 -->
        <button class="terminal-toolbar-btn" :title="isMaximized ? '还原' : '最大化'" @click="toggleMaximize">
          {{ isMaximized ? '❐' : '⬜' }}
        </button>
      </div>
    </div>

    <!-- 搜索条（TM-07） -->
    <div v-if="showSearch" class="terminal-search-bar">
      <input
        v-model="searchText"
        class="terminal-search-input"
        placeholder="搜索..."
        @keydown.enter="doSearchNext"
        @keydown.shift.enter="doSearchPrevious"
      />
      <label class="terminal-search-option">
        <input v-model="searchCaseSensitive" type="checkbox" />
        <span>区分大小写</span>
      </label>
      <button class="terminal-search-btn" @click="doSearchPrevious">↑</button>
      <button class="terminal-search-btn" @click="doSearchNext">↓</button>
      <button class="terminal-search-btn" @click="showSearch = false">✕</button>
    </div>

    <!-- 会话内容区 -->
    <div class="terminal-panel-content">
      <div
        v-for="session in sessions"
        :key="session.sessionId"
        class="terminal-panel-session"
        :class="{ active: session.sessionId === activeTabId }"
      >
        <XtermView
          :ref="(el: any) => registerXtermRef(session.sessionId, el)"
          :session-id="session.sessionId"
          :active="session.sessionId === activeTabId"
          @input="(data: string) => onXtermInput(session.sessionId, data)"
          @resize="(cols: number, rows: number) => onXtermResize(session.sessionId, cols, rows)"
        />
      </div>

      <!-- 空状态 -->
      <div v-if="sessions.length === 0" class="terminal-panel-empty">
        <span>点击 <strong>+</strong> 新建 Shell 会话</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-panel {
  position: fixed;
  bottom: var(--gw-statusbar-h);
  left: 0;
  right: 0;
  background: var(--gw-bg-panel);
  border-top: 1px solid var(--gw-border);
  display: flex;
  flex-direction: column;
  z-index: 100;
  min-height: 240px;
}

.terminal-panel-resize-handle {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 8px;
  cursor: ns-resize;
  z-index: 10;
}

.terminal-panel-resize-handle:hover {
  background: var(--gw-accent);
  opacity: 0.3;
}

.terminal-panel-content {
  flex: 1;
  position: relative;
  overflow: hidden;
}

.terminal-panel-session {
  position: absolute;
  inset: 0;
  display: none;
}

.terminal-panel-session.active {
  display: block;
}

.terminal-panel-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.terminal-panel-empty strong {
  color: var(--gw-accent);
}

/* TM-07：工具条 */
.terminal-panel-header {
  display: flex;
  align-items: center;
  border-bottom: 1px solid var(--gw-border);
  flex-shrink: 0;
}

.terminal-panel-toolbar {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 0 var(--gw-space-1);
  flex-shrink: 0;
}

.terminal-toolbar-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
  font-size: 12px;
}

.terminal-toolbar-btn:hover {
  background: var(--gw-bg-hover);
}

/* TM-07：搜索条 */
.terminal-search-bar {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  padding: var(--gw-space-1) var(--gw-space-2);
  background: var(--gw-bg-panel);
  border-bottom: 1px solid var(--gw-border);
  flex-shrink: 0;
}

.terminal-search-input {
  flex: 1;
  height: 24px;
  padding: 0 var(--gw-space-1);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-sm);
  background: var(--gw-bg-app);
  color: var(--gw-text);
  font-size: var(--gw-text-sm);
  outline: none;
}

.terminal-search-input:focus {
  border-color: var(--gw-accent);
}

.terminal-search-option {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  cursor: pointer;
}

.terminal-search-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 1px solid var(--gw-border);
  background: transparent;
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
  font-size: 12px;
  color: var(--gw-text);
}

.terminal-search-btn:hover {
  background: var(--gw-bg-hover);
}

/* TM-05：Runtime 控制按钮 */
.terminal-toolbar-btn.runtime-btn {
  font-size: 11px;
}

.terminal-toolbar-btn.runtime-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.terminal-toolbar-divider {
  width: 1px;
  height: 16px;
  background: var(--gw-border);
  margin: 0 var(--gw-space-1);
}
</style>
