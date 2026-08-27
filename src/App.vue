<template>
  <n-config-provider :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-dialog-provider>
        <div class="app-container">
          <div class="view-area">
            <router-view />
          </div>
          <!-- F-07：底部版本栏，版本号/作者构建期取自 package.json。 -->
          <footer class="app-footer">v{{ appVersion }} by {{ appAuthor }}</footer>
          <TaskPanel />
        </div>
      </n-dialog-provider>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { zhCN, dateZhCN } from "naive-ui";
import TaskPanel from "@/views/TaskPanel.vue";

// F-07：构建期注入的全局常量（vite.config.ts define，vite-env.d.ts 声明）。
const appVersion = __APP_VERSION__;
const appAuthor = __APP_AUTHOR__;
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

.app-container {
  height: 100vh;
  display: flex;
  flex-direction: column;
}

/* F-07：页面区占满剩余高度，底部版本栏固定高度。 */
.view-area {
  flex: 1;
  min-height: 0;
}

.app-footer {
  flex-shrink: 0;
  padding: 2px 12px;
  font-size: 12px;
  color: #909399;
  text-align: center;
  border-top: 1px solid #ebeef5;
  user-select: none;
}
</style>
