<template>
  <div class="panel-header">
    <div class="panel-header-title">
      <slot name="header">
        <span v-if="title" class="panel-header-text">{{ title }}</span>
      </slot>
    </div>
    <div v-if="$slots.actions" class="panel-header-actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{
  title?: string;
}>();
</script>

<style scoped>
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 36px;
  padding: 0 var(--gw-space-4);
  border-bottom: 1px solid var(--gw-border);
}

.panel-header-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  min-width: 0;
}

.panel-header-text {
  font-size: var(--gw-text-md);
  font-weight: 600;
  color: var(--gw-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.panel-header-actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  flex-shrink: 0;
}
</style>
