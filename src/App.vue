<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN" :theme="naiveTheme">
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

// D-02：主题机制
const { resolved } = useTheme();
const naiveTheme = computed(() =>
  resolved.value === "dark" ? darkTheme : null
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
