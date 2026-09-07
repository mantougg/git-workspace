<!--
  TerminalTabs — tab 条组件（TM-02，terminal-feature-plan §4.3）。
  新建 shell / 关闭 / 切换 / 会话标题与存活态。
-->

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useTerminalStore } from "@/stores/terminal";

const terminalStore = useTerminalStore();

const sessions = computed(() => terminalStore.sessions);
const activeTabId = computed(() => terminalStore.activeTabId);
const availableShells = computed(() => terminalStore.availableShells);

// TM-07：Shell profile 选择
const showShellMenu = ref(false);
const selectedShell = ref<string | null>(null);

onMounted(() => {
  terminalStore.loadShells();
});

function switchTab(sessionId: string) {
  terminalStore.switchTab(sessionId);
}

function closeTab(sessionId: string, event: Event) {
  event.stopPropagation();
  terminalStore.closeTab(sessionId);
}

function toggleShellMenu() {
  showShellMenu.value = !showShellMenu.value;
}

async function newShell(shellId?: string) {
  showShellMenu.value = false;
  const shell = shellId ?? selectedShell.value ?? undefined;
  await terminalStore.openSession(shell ? { shell } : undefined);
}

function selectShell(shellId: string) {
  selectedShell.value = shellId;
  newShell(shellId);
}
</script>

<template>
  <div class="terminal-tabs">
    <div class="terminal-tabs-list">
      <div
        v-for="session in sessions"
        :key="session.sessionId"
        class="terminal-tab"
        :class="{
          active: session.sessionId === activeTabId,
          'git-console': session.sessionId === '__git_console__',
        }"
        @click="switchTab(session.sessionId)"
      >
        <span
          v-if="session.sessionId !== '__git_console__'"
          class="terminal-tab-dot"
          :class="{ alive: session.alive }"
        />
        <span v-else class="terminal-tab-icon">🔀</span>
        <span class="terminal-tab-title">{{ session.title }}</span>
        <button
          v-if="session.sessionId !== '__git_console__'"
          class="terminal-tab-close"
          title="关闭"
          @click="closeTab(session.sessionId, $event)"
        >
          ×
        </button>
      </div>
    </div>
    <!-- TM-07：新建 Shell 按钮 + profile 下拉 -->
    <div class="terminal-new-shell-wrapper">
      <button class="terminal-tabs-new" title="新建 Shell" @click="toggleShellMenu">
        +
      </button>
      <div v-if="showShellMenu" class="terminal-shell-menu">
        <div
          v-for="shell in availableShells"
          :key="shell.id"
          class="terminal-shell-item"
          @click="selectShell(shell.id)"
        >
          <span class="terminal-shell-label">{{ shell.label }}</span>
          <span class="terminal-shell-path">{{ shell.path }}</span>
        </div>
        <div v-if="availableShells.length === 0" class="terminal-shell-item disabled">
          未探测到可用 Shell
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.terminal-tabs {
  display: flex;
  align-items: center;
  height: 32px;
  background: var(--gw-bg-panel);
  border-bottom: 1px solid var(--gw-border);
  padding: 0 var(--gw-space-1);
  gap: var(--gw-space-1);
  overflow-x: auto;
  flex-shrink: 0;
}

.terminal-tabs-list {
  display: flex;
  gap: 2px;
  flex: 1;
  overflow-x: auto;
}

.terminal-tab {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  padding: 0 var(--gw-space-2);
  height: 26px;
  border-radius: var(--gw-radius-sm);
  cursor: pointer;
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
  white-space: nowrap;
  flex-shrink: 0;
  transition: background 0.15s;
}

.terminal-tab:hover {
  background: var(--gw-bg-hover);
}

.terminal-tab.active {
  background: var(--gw-bg-hover);
  color: var(--gw-text);
}

.terminal-tab-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--gw-text-dim);
  flex-shrink: 0;
}

.terminal-tab-dot.alive {
  background: var(--gw-success);
}

.terminal-tab-icon {
  font-size: 10px;
  flex-shrink: 0;
}

.terminal-tab.git-console {
  border-left: 2px solid var(--gw-accent);
}

.terminal-tab-title {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.terminal-tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: var(--gw-text-dim);
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
  font-size: 14px;
  line-height: 1;
  padding: 0;
}

.terminal-tab-close:hover {
  background: var(--gw-danger);
  color: #fff;
}

.terminal-tabs-new {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: 1px solid var(--gw-border);
  background: transparent;
  color: var(--gw-text-dim);
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
  font-size: 16px;
  flex-shrink: 0;
}

.terminal-tabs-new:hover {
  background: var(--gw-bg-hover);
  color: var(--gw-text);
}

/* TM-07：Shell profile 下拉菜单 */
.terminal-new-shell-wrapper {
  position: relative;
}

.terminal-shell-menu {
  position: absolute;
  top: 100%;
  right: 0;
  z-index: 200;
  min-width: 200px;
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-sm);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  padding: var(--gw-space-1) 0;
}

.terminal-shell-item {
  display: flex;
  flex-direction: column;
  padding: var(--gw-space-1) var(--gw-space-2);
  cursor: pointer;
  gap: 2px;
}

.terminal-shell-item:hover:not(.disabled) {
  background: var(--gw-bg-hover);
}

.terminal-shell-item.disabled {
  color: var(--gw-text-dim);
  cursor: not-allowed;
}

.terminal-shell-label {
  font-size: var(--gw-text-sm);
  color: var(--gw-text);
}

.terminal-shell-path {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
}
</style>
