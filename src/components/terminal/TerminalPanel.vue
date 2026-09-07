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

    <!-- Tab 条 -->
    <TerminalTabs />

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
</style>
