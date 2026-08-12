<template>
  <div class="status-badge">
    <el-tag v-if="status.isClean" type="success" size="small" effect="plain">
      Clean
    </el-tag>
    <template v-else>
      <el-tag v-if="status.modified > 0" type="warning" size="small">
        M {{ status.modified }}
      </el-tag>
      <el-tag v-if="status.added > 0" type="success" size="small">
        A {{ status.added }}
      </el-tag>
      <el-tag v-if="status.deleted > 0" type="danger" size="small">
        D {{ status.deleted }}
      </el-tag>
      <el-tag v-if="status.untracked > 0" type="info" size="small">
        U {{ status.untracked }}
      </el-tag>
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
