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
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from "vue";
import { zhCN, dateZhCN, darkTheme } from "naive-ui";
import AppShell from "@/components/shell/AppShell.vue";
import TaskPanel from "@/views/TaskPanel.vue";
import CommandPalette from "@/components/shell/CommandPalette.vue";
import { useTheme } from "@/composables/useTheme";
import { lightOverrides, darkOverrides } from "@/styles/naive-overrides";

// D-02：主题机制
const { resolved } = useTheme();
const naiveTheme = computed(() =>
  resolved.value === "dark" ? darkTheme : null
);

// D-07：组件级 themeOverrides
const themeOverrides = computed(() =>
  resolved.value === "dark" ? darkOverrides : lightOverrides
);

// D-12：Command Palette
const showPalette = ref(false);

function onGlobalKeydown(e: KeyboardEvent) {
  if ((e.metaKey || e.ctrlKey) && e.key === "k") {
    e.preventDefault();
    showPalette.value = !showPalette.value;
  }
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
