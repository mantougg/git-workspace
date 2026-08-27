<template>
  <n-card class="repo-card" hoverable @click="emit('click', repo)">
    <div class="card-header">
      <n-icon v-if="repo.repository.isFavorite" color="var(--gw-warning)">
        <StarOutline />
      </n-icon>
      <span class="card-title">{{ repo.repository.name }}</span>
      <StatusBadge v-if="repo.status" :status="repo.status" />
    </div>
    <div class="card-body">
      <div class="card-path">{{ repo.repository.relativePath }}</div>
      <div v-if="repo.status" class="card-meta">
        <n-tag size="small" :bordered="false">
          {{ repo.status.branch }}
        </n-tag>
        <span v-if="repo.status.ahead > 0" class="text-success">
          ↑{{ repo.status.ahead }}
        </span>
        <span v-if="repo.status.behind > 0" class="text-warning">
          ↓{{ repo.status.behind }}
        </span>
      </div>
    </div>
  </n-card>
</template>

<script setup lang="ts">
import { StarOutline } from "@vicons/ionicons5";
import type { RepositoryWithStatus } from "@/types/repository";
import StatusBadge from "./StatusBadge.vue";

defineProps<{
  repo: RepositoryWithStatus;
}>();

const emit = defineEmits<{
  (e: "click", repo: RepositoryWithStatus): void;
}>();
</script>

<style scoped>
.repo-card {
  cursor: pointer;
  transition: transform 0.2s;
}

.repo-card:hover {
  transform: translateY(-2px);
}

.card-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: 8px;
}

.card-title {
  font-weight: 600;
  font-size: 14px;
  flex: 1;
}

.card-body {
  font-size: 13px;
}

.card-path {
  color: var(--gw-text-dim);
  margin-bottom: 4px;
}

.card-meta {
  display: flex;
  align-items: center;
  gap: 6px;
}

.text-success {
  color: var(--gw-success);
}

.text-warning {
  color: var(--gw-warning);
}
</style>
