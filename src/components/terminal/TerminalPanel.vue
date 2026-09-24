<!--
  TerminalPanel — 底部 drawer + tab 容器（TM-02，terminal-feature-plan §4.3）。
  仿 TaskPanel.vue：底部、可拖高 240px~85vh，StatusBar 开合。
  挂载进 App.vue 全局 overlay 区。
-->

<script setup lang="ts">
import { ref, computed, watch, nextTick, onMounted, onBeforeUnmount } from "vue";
import { useMessage } from "naive-ui";
import { open as shellOpen } from "@tauri-apps/plugin-shell";
import { homeDir, join } from "@tauri-apps/api/path";
import { useTerminalStore } from "@/stores/terminal";
import { encodeUtf8Base64 } from "@/utils/base64";
import { errMsg } from "@/utils/error";
import { openGitCredentialManager } from "@/api/integration";
import TerminalTabs from "./TerminalTabs.vue";
import XtermView from "./XtermView.vue";

const terminalStore = useTerminalStore();
const message = useMessage();
const activeSession = computed(() => terminalStore.activeSession);

// GF-08：Git Console 失败引导（最近一次失败的单仓网络操作的分类）。
const gitOpGuidance = computed(
  () => terminalStore.lastGitOpFailure?.guidance ?? null
);

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
/** PAF-14：聚焦用模板 ref，querySelector 选择器与模板 class 位置不符永远匹配不到 */
const searchInputRef = ref<HTMLInputElement | null>(null);

function toggleSearch() {
  showSearch.value = !showSearch.value;
  if (showSearch.value) {
    nextTick(() => searchInputRef.value?.focus());
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
// TM-07：右键上下文菜单
// ---------------------------------------------------------------------------
const contextMenu = ref({
  show: false,
  x: 0,
  y: 0,
});

function onContextMenu(e: MouseEvent) {
  e.preventDefault();
  contextMenu.value = {
    show: true,
    x: e.clientX,
    y: e.clientY,
  };
}

function hideContextMenu() {
  contextMenu.value.show = false;
}

async function contextCopy() {
  hideContextMenu();
  // 使用 xterm 的 API 获取选中文本（比 window.getSelection 更可靠）
  const xtermRef = activeTabId.value ? xtermRefs.value.get(activeTabId.value) : null;
  const selection = xtermRef?.getTerminal()?.getSelection();
  if (selection) {
    await navigator.clipboard.writeText(selection);
  }
}

async function contextPaste() {
  hideContextMenu();
  const text = await navigator.clipboard.readText();
  if (text && activeTabId.value) {
    await terminalStore.writeToSession(activeTabId.value, encodeUtf8Base64(text));
  }
}

function contextClear() {
  hideContextMenu();
  clearScreen();
}

async function contextCloseTab() {
  hideContextMenu();
  if (activeTabId.value) {
    await terminalStore.closeTab(activeTabId.value);
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
  window.removeEventListener("terminal:toggle-search", toggleSearch);
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
  // PAF-14：命令面板「终端内搜索」经 window 自定义事件接线到本面板
  // （此前 registry.ts 派发后全工程无人监听，命令失效）。
  window.addEventListener("terminal:toggle-search", toggleSearch);
});

// ---------------------------------------------------------------------------
// GF-08：Git Console 失败引导动作
// ---------------------------------------------------------------------------

/** 打开系统凭据管理器（Windows 控制面板 / macOS 钥匙串 / Linux Seahorse）。 */
async function openCredentialManager() {
  try {
    await openGitCredentialManager();
  } catch (e) {
    message.error("打开系统凭据管理器失败：" + errMsg(e));
  }
}

/** 打开用户目录下的 SSH key 配置目录（~/.ssh）。 */
async function openSshKeyDir() {
  try {
    const dir = await join(await homeDir(), ".ssh");
    await shellOpen(dir);
  } catch (e) {
    message.error("打开 SSH key 目录失败：" + errMsg(e));
  }
}
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
        <!-- GF-07：单仓网络操作取消入口（sync_*/push_branch；与 TaskPanel 取消体验对齐） -->
        <template v-if="terminalStore.gitOpsInFlight.length > 0">
          <n-button
            v-for="op in terminalStore.gitOpsInFlight"
            :key="op.opId"
            size="small"
            text
            type="error"
            :title="`取消 ${op.command}（${op.repoName}）`"
            @click="terminalStore.cancelGitOp(op.opId)"
          >
            取消
          </n-button>
          <div class="terminal-toolbar-divider" />
        </template>
        <!-- GF-08：Git Console 失败引导（认证失败等分类 → 动作入口） -->
        <template v-if="isGitConsoleTab && gitOpGuidance">
          <n-popover trigger="click" placement="top">
            <template #trigger>
              <n-button
                size="small"
                text
                type="warning"
                :title="gitOpGuidance.reason || 'Git 操作失败引导'"
              >
                {{ gitOpGuidance.category === "authentication" ? "认证引导" : "失败引导" }}
              </n-button>
            </template>
            <div class="git-op-guidance">
              <div v-if="gitOpGuidance.reason" class="git-op-guidance-reason">
                {{ gitOpGuidance.reason }}
              </div>
              <div
                v-if="gitOpGuidance.category === 'authentication'"
                class="git-op-guidance-actions"
              >
                <n-button size="tiny" @click="openCredentialManager">
                  打开系统凭据管理器
                </n-button>
                <n-button size="tiny" @click="openSshKeyDir">打开 SSH key 目录</n-button>
              </div>
              <ul v-if="gitOpGuidance.actions.length" class="git-op-guidance-list">
                <li v-for="action in gitOpGuidance.actions" :key="action">{{ action }}</li>
              </ul>
            </div>
          </n-popover>
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
        ref="searchInputRef"
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
    <div class="terminal-panel-content" @contextmenu="onContextMenu">
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

    <!-- TM-07：右键上下文菜单 -->
    <Teleport to="body">
      <div
        v-if="contextMenu.show"
        class="terminal-context-menu"
        :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
        @click="hideContextMenu"
      >
        <div class="terminal-context-item" @click="contextCopy">
          <span class="terminal-context-icon">📋</span>
          <span>复制</span>
          <span class="terminal-context-shortcut">Ctrl+Shift+C</span>
        </div>
        <div class="terminal-context-item" @click="contextPaste">
          <span class="terminal-context-icon">📄</span>
          <span>粘贴</span>
          <span class="terminal-context-shortcut">Ctrl+Shift+V</span>
        </div>
        <div class="terminal-context-divider" />
        <div class="terminal-context-item" @click="contextClear">
          <span class="terminal-context-icon">🗑</span>
          <span>清屏</span>
        </div>
        <div
          v-if="activeTabId"
          class="terminal-context-item"
          @click="contextCloseTab"
        >
          <span class="terminal-context-icon">✕</span>
          <span>关闭 Tab</span>
        </div>
      </div>
    </Teleport>
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
  margin-left: auto;
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

/* GF-08：Git Console 失败引导气泡 */
.git-op-guidance {
  max-width: 320px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.git-op-guidance-reason {
  font-size: var(--gw-text-sm);
  color: var(--gw-text);
}

.git-op-guidance-actions {
  display: flex;
  gap: var(--gw-space-1);
  flex-wrap: wrap;
}

.git-op-guidance-list {
  margin: 0;
  padding-left: 18px;
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.git-op-guidance-list li {
  word-break: break-all;
}

/* TM-07：右键上下文菜单 */
.terminal-context-menu {
  position: fixed;
  z-index: 1000;
  min-width: 180px;
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-sm);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  padding: var(--gw-space-1) 0;
}

.terminal-context-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: var(--gw-space-1) var(--gw-space-2);
  cursor: pointer;
  font-size: var(--gw-text-sm);
  color: var(--gw-text);
}

.terminal-context-item:hover {
  background: var(--gw-bg-hover);
}

.terminal-context-icon {
  width: 20px;
  text-align: center;
  font-size: 12px;
}

.terminal-context-shortcut {
  margin-left: auto;
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.terminal-context-divider {
  height: 1px;
  background: var(--gw-border);
  margin: var(--gw-space-1) 0;
}

</style>
