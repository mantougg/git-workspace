<template>
  <div class="app-container">
    <router-view />
    <TaskPanel />
  </div>
</template>

<script setup lang="ts">
import TaskPanel from "@/views/TaskPanel.vue";
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
</style>
