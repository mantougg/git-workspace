<template>
  <n-dropdown
    :show="show"
    :options="options"
    :x="x"
    :y="y"
    trigger="manual"
    placement="bottom-start"
    @select="onSelect"
    @clickoutside="onClose"
  />
</template>

<script setup lang="ts">
import { NDropdown } from "naive-ui";
import type { DropdownOption } from "naive-ui";

defineProps<{
  show: boolean;
  options: DropdownOption[];
  x: number;
  y: number;
}>();

const emit = defineEmits<{
  (e: "select", key: string): void;
  (e: "close"): void;
}>();

function onSelect(key: string) {
  emit("select", key);
  emit("close");
}

function onClose() {
  emit("close");
}
</script>
