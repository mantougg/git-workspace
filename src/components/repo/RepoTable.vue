<template>
  <n-data-table
    :columns="columns"
    :data="data"
    :loading="loading ?? false"
    :row-key="(row: RepositoryWithStatus) => row.repository.path"
    :checked-row-keys="checkedKeys"
    :scroll-x="800"
    :max-height="height ? Number(height) : undefined"
    @update:checked-row-keys="onSelectionChange"
    @row-click="(row: RepositoryWithStatus) => emit('row-click', row)"
    style="width: 100%"
  />
</template>

<script setup lang="ts">
import { h, ref } from "vue";
import { NButton, NIcon, NTag } from "naive-ui";
import { StarOutline } from "@vicons/ionicons5";
import type { DataTableColumns } from "naive-ui";
import type { RepositoryWithStatus } from "@/types/repository";
import StatusBadge from "./StatusBadge.vue";

const props = defineProps<{
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

const checkedKeys = ref<(string | number)[]>([]);

function onSelectionChange(keys: (string | number)[]) {
  checkedKeys.value = keys;
  const selected = props.data.filter((row) =>
    keys.includes(row.repository.path),
  );
  emit("selection-change", selected);
}

const columns: DataTableColumns<RepositoryWithStatus> = [
  {
    type: "selection",
  },
  {
    title: "仓库",
    key: "repository.name",
    minWidth: 180,
    sorter: true,
    render(row) {
      const parts = [];
      if (row.repository.isFavorite) {
        parts.push(
          h(NIcon, { color: "#e6a23c", size: 14 }, () => h(StarOutline)),
        );
      }
      parts.push(h("span", null, row.repository.name));
      return h("div", [
        h("div", { class: "repo-name" }, parts),
        h("div", { class: "repo-path" }, row.repository.relativePath),
      ]);
    },
  },
  {
    title: "分支",
    key: "status.branch",
    width: 140,
    render(row) {
      if (row.status?.isDetached) {
        return h(NTag, { type: "warning", size: "small" }, () => row.status?.branch);
      }
      if (row.status) {
        return h("span", null, row.status.branch);
      }
      return h("span", { class: "text-muted" }, "-");
    },
  },
  {
    title: "状态",
    key: "status",
    minWidth: 160,
    render(row) {
      if (row.status) {
        return h(StatusBadge, { status: row.status });
      }
      if (row.lastError) {
        return h(NTag, { type: "error", size: "small" }, () => "错误");
      }
      return h("span", { class: "text-muted" }, "-");
    },
  },
  {
    title: "远程",
    key: "remote",
    width: 120,
    render(row) {
      if (row.status && (row.status.ahead > 0 || row.status.behind > 0)) {
        return h("div", null, [
          h(
            "span",
            { class: row.status.ahead > 0 ? "text-success" : "" },
            `↑${row.status.ahead}`,
          ),
          h("span", { class: "separator" }, "/"),
          h(
            "span",
            { class: row.status.behind > 0 ? "text-warning" : "" },
            `↓${row.status.behind}`,
          ),
        ]);
      }
      if (row.status) {
        return h("span", { class: "text-muted" }, "同步");
      }
      return null;
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 160,
    align: "center",
    render(row) {
      return h("div", { style: "display: flex; gap: 4px; justify-content: center" }, [
        h(
          NButton,
          {
            size: "small",
            text: true,
            type: "primary",
            disabled: !row.status || row.status.isClean,
            onClick: (e: Event) => {
              e.stopPropagation();
              emit("view-diff", row.repository.path);
            },
          },
          () => "Diff",
        ),
        h(
          NButton,
          {
            size: "small",
            text: true,
            type: "success",
            onClick: (e: Event) => {
              e.stopPropagation();
              emit("view-graph", row.repository.path);
            },
          },
          () => "Graph",
        ),
      ]);
    },
  },
];
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
</style>
