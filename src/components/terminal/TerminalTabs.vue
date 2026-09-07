<!--
  TerminalTabs — tab 条组件（TM-02，terminal-feature-plan §4.3）。
  新建 shell / 关闭 / 切换 / 会话标题与存活态。
-->

<script setup lang="ts">
import { computed } from "vue";
import { useTerminalStore } from "@/stores/terminal";

const terminalStore = useTerminalStore();

const sessions = computed(() => terminalStore.sessions);
const activeTabId = computed(() => terminalStore.activeTabId);

function switchTab(sessionId: string) {
  terminalStore.switchTab(sessionId);
}

function closeTab(sessionId: string, event: Event) {
  event.stopPropagation();
  terminalStore.closeTab(sessionId);
}

async function newShell() {
  await terminalStore.openSession();
}
</script>

<template>
  <div class="terminal-tabs">
    <div class="terminal-tabs-list">
      <div
        v-for="session in sessions"
        :key="session.sessionId"
        class="terminal-tab"
        :class="{ active: session.sessionId === activeTabId }"
        @click="switchTab(session.sessionId)"
      >
        <span class="terminal-tab-dot" :class="{ alive: session.alive }" />
        <span class="terminal-tab-title">{{ session.title }}</span>
        <button
          class="terminal-tab-close"
          title="关闭"
          @click="closeTab(session.sessionId, $event)"
        >
          ×
        </button>
      </div>
    </div>
    <button class="terminal-tabs-new" title="新建 Shell" @click="newShell">
      +
    </button>
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
</style>
