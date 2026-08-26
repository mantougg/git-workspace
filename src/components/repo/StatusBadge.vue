<template>
  <div class="status-badge">
    <n-tag v-if="status.isClean" type="success" size="small" :bordered="false">
      Clean
    </n-tag>
    <template v-else>
      <n-tag v-if="status.modified > 0" type="warning" size="small">
        M {{ status.modified }}
      </n-tag>
      <n-tag v-if="status.added > 0" type="success" size="small">
        A {{ status.added }}
      </n-tag>
      <n-tag v-if="status.deleted > 0" type="error" size="small">
        D {{ status.deleted }}
      </n-tag>
      <n-tag v-if="status.untracked > 0" type="info" size="small">
        U {{ status.untracked }}
      </n-tag>
    </template>
  </div>
</template>

<script setup lang="ts">
import type { RepoStatus } from "@/types/repository";

defineProps<{
  status: RepoStatus;
}>();
</script>

<style scoped>
.status-badge {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}
</style>
