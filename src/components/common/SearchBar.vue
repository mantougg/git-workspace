<template>
  <div class="search-bar">
    <el-input
      v-model="model"
      :placeholder="placeholder"
      style="width: 240px"
      clearable
      :prefix-icon="Search"
      @keyup.enter="emit('search', model)"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from "vue";
import { Search } from "@element-plus/icons-vue";

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
