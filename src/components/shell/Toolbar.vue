<template>
  <div class="toolbar">
    <div class="toolbar-left">
      <slot />
    </div>
    <div v-if="$slots.right" class="toolbar-right">
      <slot name="right" />
    </div>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-2);
  padding: var(--gw-space-2) 0;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}
</style>
