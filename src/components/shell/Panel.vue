<template>
  <div class="panel">
    <PanelHeader v-if="title || $slots.header" :title="title">
      <template v-if="$slots.header" #header>
        <slot name="header" />
      </template>
      <template v-if="$slots.actions" #actions>
        <slot name="actions" />
      </template>
    </PanelHeader>
    <div class="panel-body">
      <slot />
    </div>
  </div>
</template>

<script setup lang="ts">
import PanelHeader from "./PanelHeader.vue";

defineProps<{
  title?: string;
}>();
</script>

<style scoped>
.panel {
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  overflow: hidden;
}

.panel-body {
  padding: var(--gw-space-3) var(--gw-space-4);
}
</style>
