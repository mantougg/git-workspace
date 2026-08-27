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
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { zhCN, dateZhCN, darkTheme } from "naive-ui";
import AppShell from "@/components/shell/AppShell.vue";
import TaskPanel from "@/views/TaskPanel.vue";
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
