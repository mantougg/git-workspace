<template>
  <n-config-provider
    :locale="zhCN"
    :date-locale="dateZhCN"
    :theme="naiveTheme"
    :theme-overrides="themeOverrides"
  >
    <n-message-provider>
      <n-dialog-provider>
        <AppShell>
          <router-view />
        </AppShell>
        <TaskPanel />
        <CommandPalette v-model:show="showPalette" />
        <!-- AI-10：全局唯一 Assistant Drawer（会话状态在 stores/ai.ts） -->
        <AssistantDrawer />
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import { zhCN, dateZhCN, darkTheme } from "naive-ui";
import AppShell from "@/components/shell/AppShell.vue";
import TaskPanel from "@/views/TaskPanel.vue";
import CommandPalette from "@/components/shell/CommandPalette.vue";
import AssistantDrawer from "@/components/ai/AssistantDrawer.vue";
import { useTheme } from "@/composables/useTheme";
import { lightOverrides, darkOverrides } from "@/styles/naive-overrides";
import { createShortcutListener } from "@/commands/shortcuts";
import { getAllCommands } from "@/commands/registry";
import type { CommandContext } from "@/commands/registry";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useAiStore } from "@/stores/ai";

// D-02：主题机制
const { resolved } = useTheme();
const naiveTheme = computed(() =>
  resolved.value === "dark" ? darkTheme : null
);

// D-07：组件级 themeOverrides
const themeOverrides = computed(() =>
  resolved.value === "dark" ? darkOverrides : lightOverrides
);

// D-12/D-14 + T-31：Command Palette + 快捷键。
// 命令上下文在 setup 期构建：keydown 事件上下文拿不到组件实例，
// useRouter() / useXxxStore() 必须提前解析。
const commandContext: CommandContext = {
  router: useRouter(),
  workspaceStore: useWorkspaceStore(),
  repoStore: useRepositoryStore(),
  aiStore: useAiStore(),
};

const showPalette = ref(false);
const shortcutListener = createShortcutListener(() =>
  getAllCommands(commandContext)
);

function onGlobalKeydown(e: KeyboardEvent) {
  // Command Palette 快捷键优先（Ctrl+K / Ctrl+Shift+P）
  if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
    e.preventDefault();
    showPalette.value = !showPalette.value;
    return;
  }
  if ((e.metaKey || e.ctrlKey) && e.shiftKey && (e.key === "P" || e.key === "p")) {
    e.preventDefault();
    showPalette.value = !showPalette.value;
    return;
  }
  // 其他快捷键走命令注册表
  shortcutListener(e);
}

onMounted(() => {
  document.addEventListener("keydown", onGlobalKeydown);
});

onUnmounted(() => {
  document.removeEventListener("keydown", onGlobalKeydown);
});
</script>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html,
body,
#app {
  height: 100%;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Oxygen,
    Ubuntu, Cantarell, "Open Sans", "Helvetica Neue", sans-serif;
}
</style>
