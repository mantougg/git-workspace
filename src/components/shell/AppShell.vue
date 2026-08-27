<template>
  <div class="app-shell">
    <SideNav />
    <main class="app-shell-content">
      <slot />
    </main>
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

.app-shell-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  background: var(--gw-bg-app);
}
</style>
