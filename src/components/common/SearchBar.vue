<template>
  <div class="search-bar">
    <n-input
      v-model:value="model"
      :placeholder="placeholder"
      style="width: 240px"
      clearable
      @keyup.enter="emit('search', model)"
    >
      <template #prefix>
        <n-icon><SearchOutline /></n-icon>
      </template>
    </n-input>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { SearchOutline } from "@vicons/ionicons5";

const props = withDefaults(
  defineProps<{
    modelValue?: string;
    placeholder?: string;
  }>(),
  {
    modelValue: "",
    placeholder: "搜索...",
  },
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "search", value: string): void;
}>();

const model = ref(props.modelValue);

watch(
  () => props.modelValue,
  (val) => {
    model.value = val;
  },
);

watch(model, (val) => {
  emit("update:modelValue", val);
});
</script>

<style scoped>
.search-bar {
  display: inline-flex;
  align-items: center;
}
</style>
