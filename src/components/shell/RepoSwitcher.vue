<template>
  <!-- F-22：Git 视图头部仓库切换器。数据源为当前工作区仓库列表
       （repoStore.repositories，由各视图 onMounted 的 resolveCurrentRepo 保证
       已加载）；值绑定全局当前仓库，切换后 emit change 由视图重置并重载。 -->
  <n-select
    class="repo-switcher"
    :value="repoStore.currentRepoPath || null"
    :options="options"
    :title="repoStore.currentRepoPath"
    size="small"
    filterable
    :consistent-menu-width="false"
    placeholder="选择仓库"
    @update:value="onUpdate"
  />
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useRepositoryStore } from "@/stores/repository";

const emit = defineEmits<{
  (e: "change", path: string): void;
}>();

const repoStore = useRepositoryStore();

const options = computed(() =>
  repoStore.repositories.map((r) => ({
    label: r.repository.name,
    value: r.repository.path,
  })),
);

function onUpdate(value: string | null) {
  if (!value) return;
  repoStore.setCurrentRepoPath(value);
  emit("change", value);
}
</script>

<style scoped>
.repo-switcher {
  width: 220px;
}
</style>
