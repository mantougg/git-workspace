<template>
  <el-table
    :data="data"
    v-loading="loading ?? false"
    stripe
    highlight-current-row
    @row-click="(row: any) => emit('row-click', row)"
    @selection-change="(sel: any[]) => emit('selection-change', sel)"
    style="width: 100%"
    :height="height"
  >
    <el-table-column type="selection" width="40" />
    <el-table-column label="仓库" min-width="180" sortable>
      <template #default="{ row }">
        <div class="repo-name">
          <el-icon v-if="row.repository.isFavorite" color="#e6a23c">
            <Star />
          </el-icon>
          <span>{{ row.repository.name }}</span>
        </div>
        <div class="repo-path">{{ row.repository.relativePath }}</div>
      </template>
    </el-table-column>
    <el-table-column label="分支" width="140">
      <template #default="{ row }">
        <el-tag v-if="row.status?.isDetached" type="warning" size="small">
          {{ row.status?.branch }}
        </el-tag>
        <span v-else-if="row.status">{{ row.status.branch }}</span>
        <span v-else class="text-muted">-</span>
      </template>
    </el-table-column>
    <el-table-column label="状态" min-width="160">
      <template #default="{ row }">
        <StatusBadge v-if="row.status" :status="row.status" />
        <el-tag v-else-if="row.lastError" type="danger" size="small">
          错误
        </el-tag>
        <span v-else class="text-muted">-</span>
      </template>
    </el-table-column>
    <el-table-column label="远程" width="120">
      <template #default="{ row }">
        <div v-if="row.status && (row.status.ahead > 0 || row.status.behind > 0)">
          <span :class="{ 'text-success': row.status.ahead > 0 }">
            ↑{{ row.status.ahead }}
          </span>
          <span class="separator">/</span>
          <span :class="{ 'text-warning': row.status.behind > 0 }">
            ↓{{ row.status.behind }}
          </span>
        </div>
        <span v-else-if="row.status" class="text-muted">同步</span>
      </template>
    </el-table-column>
    <el-table-column label="操作" width="160" align="center">
      <template #default="{ row }">
        <el-button
          size="small"
          link
          type="primary"
          :disabled="!row.status || row.status.isClean"
          @click.stop="emit('view-diff', row.repository.path)"
        >
          Diff
        </el-button>
        <el-button
          size="small"
          link
          type="success"
          @click.stop="emit('view-graph', row.repository.path)"
        >
          Graph
        </el-button>
      </template>
    </el-table-column>
  </el-table>
</template>

<script setup lang="ts">
import { Star } from "@element-plus/icons-vue";
import type { RepositoryWithStatus } from "@/types/repository";
import StatusBadge from "./StatusBadge.vue";

defineProps<{
  data: RepositoryWithStatus[];
  loading?: boolean;
  height?: string;
}>();

const emit = defineEmits<{
  (e: "row-click", row: any): void;
  (e: "selection-change", selection: any[]): void;
  (e: "view-diff", repoPath: string): void;
  (e: "view-graph", repoPath: string): void;
}>();
</script>

<style scoped>
.repo-name {
  display: flex;
  align-items: center;
  gap: 4px;
  font-weight: 500;
}

.repo-path {
  font-size: 12px;
  color: #909399;
  margin-top: 2px;
}

.text-muted {
  color: #c0c4cc;
}

.text-success {
  color: #67c23a;
}

.text-warning {
  color: #e6a23c;
}

.separator {
  margin: 0 4px;
  color: #dcdfe6;
}

:deep(.el-table__row) {
  cursor: pointer;
}
</style>
