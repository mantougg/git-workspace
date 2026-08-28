<template>
  <div class="app-shell">
    <!-- §5.1：SideNav 与内容区是「行」向并排，StatusBar 通栏在底部 -->
    <div class="app-shell-body">
      <SideNav />
      <main class="app-shell-content">
        <slot />
      </main>
    </div>
    <StatusBar />
  </div>
</template>

<script setup lang="ts">
import SideNav from "./SideNav.vue";
import StatusBar from "./StatusBar.vue";
</script>

<style scoped>
.app-shell {
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--gw-bg-app);
}

/* F-13：行向容器承载 SideNav + 内容区。缺了它时 SideNav 的 height:100%
   会在列向 flex 里吃掉全部高度，把内容区挤到 0 高（页面只剩左侧菜单）。 */
.app-shell-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: row;
}

.app-shell-content {
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow-y: auto;
  background: var(--gw-bg-app);
}
</style>
