<template>
  <div class="operation-log-view">
    <!-- Top toolbar: filters (workspace / op type / repo / date range) -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-select
          v-model:value="filterWorkspaceId"
          placeholder="全部工作区"
          clearable
          style="width: 160px"
          :options="workspaceOptions"
          @update:value="reload"
        />
        <n-select
          v-model:value="filterOpType"
          placeholder="全部操作类型"
          clearable
          style="width: 170px"
          :options="opTypeOptions"
          @update:value="reload"
        />
        <n-input
          v-model:value="filterRepo"
          placeholder="按仓库路径筛选"
          clearable
          style="width: 200px"
          @change="reload"
          @clear="reload"
        />
        <n-date-picker
          v-model:formatted-value="dateRange"
          type="daterange"
          value-format="yyyy-MM-DD"
          start-placeholder="开始日期"
          end-placeholder="结束日期"
          style="width: 240px"
          @update:formatted-value="reload"
        />
        <n-button :loading="loading" @click="reload">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
    </div>

    <!-- Log table; expanding a row lazy-loads its per-repo ref snapshots -->
    <n-spin :show="loading">
      <n-data-table
        :columns="columns"
        :data="logs"
        size="small"
        :row-key="(row: any) => row.id"
        :expanded-row-keys="expandedRowKeys"
        @update:expanded-keys="onExpandChange"
      />
    </n-spin>

    <div class="pager">
      <n-pagination
        v-model:page="page"
        :item-count="total"
        :page-size="pageSize"
        @update:page="reload"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { h, onMounted, ref } from "vue";
import { RefreshOutline } from "@vicons/ionicons5";
import { NButton, NIcon, NTag, useMessage, useDialog } from "naive-ui";
import { useWorkspaceStore } from "@/stores/workspace";
import {
  getOperationLogDetail,
  listOperationLogs,
  previewUndoOperation,
  undoOperation,
} from "@/api/operationLog";
import type {
  OperationLogItem,
  OperationLogSummary,
  UndoPreviewItem,
} from "@/types/operationLog";
import { errMsg } from "@/utils/error";
import { formatDate } from "@/utils/format";

const workspaceStore = useWorkspaceStore();
const message = useMessage();
const dialog = useDialog();

// --- Filters / paging ---
const filterWorkspaceId = ref<number | null>(null);
const filterOpType = ref<string | null>(null);
const filterRepo = ref("");
const dateRange = ref<[string, string] | null>(null);
const page = ref(1);
const pageSize = 50;
const total = ref(0);
const logs = ref<OperationLogSummary[]>([]);
const loading = ref(false);

// --- Expanded-row details (lazy loaded per log id) ---
const details = ref<Record<number, OperationLogItem[]>>({});
const detailLoading = ref<Record<number, boolean>>({});
const expandedRowKeys = ref<number[]>([]);

const undoingId = ref<number | null>(null);

const workspaceOptions = workspaceStore.workspaces.map((ws) => ({
  label: ws.name,
  value: ws.id,
}));

/** op_type → display meta (all four logged kinds are reversible). */
const OP_TYPE_META: Record<string, { label: string; tag: "warning" | "error" | "info" }> = {
  checkout_all: { label: "批量检出", tag: "warning" },
  delete_branch_all: { label: "批量删除分支", tag: "error" },
  reset: { label: "Reset", tag: "error" },
  rebase: { label: "Rebase", tag: "warning" },
};
const opTypeOptions = Object.entries(OP_TYPE_META).map(([value, m]) => ({
  value,
  label: m.label,
}));

function opTypeMeta(opType: string) {
  return OP_TYPE_META[opType] ?? { label: opType, tag: "info" as const };
}

function statusOf(row: OperationLogSummary): { label: string; type: "info" | "warning" | "success" } {
  if (row.undoneAt) return { label: "已撤销", type: "info" };
  if (row.undoneCount > 0) return { label: "部分撤销", type: "warning" };
  return { label: "可撤销", type: "success" };
}

function repoName(p: string): string {
  return p.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? p;
}

function shortOid(oid: string): string {
  return oid.slice(0, 7);
}

/** Translate machine details ("mode:hard" / "onto:x") for display. */
function formatDetail(detail: string | null): string {
  if (!detail) return "—";
  if (detail.startsWith("mode:")) return `模式：${detail.slice(5)}`;
  if (detail.startsWith("onto:")) return `onto：${detail.slice(5)}`;
  return detail;
}

async function reload() {
  loading.value = true;
  try {
    const res = await listOperationLogs(
      filterWorkspaceId.value,
      filterRepo.value.trim() || null,
      filterOpType.value,
      dateRange.value?.[0] ?? null,
      dateRange.value?.[1] ?? null,
      pageSize,
      (page.value - 1) * pageSize,
    );
    logs.value = res.logs;
    total.value = res.total;
    // Snapshots may have changed (undo writes back) — drop cached details.
    details.value = {};
  } catch (e) {
    message.error("加载操作日志失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

/** Lazy-load the per-repo ref snapshots when a row is expanded. */
function onExpandChange(keys: number[]) {
  const newKey = keys.find((k) => !expandedRowKeys.value.includes(k));
  expandedRowKeys.value = keys;
  if (newKey != null && !details.value[newKey]) {
    loadDetail(newKey);
  }
}

async function loadDetail(id: number) {
  detailLoading.value[id] = true;
  try {
    const d = await getOperationLogDetail(id);
    details.value[id] = d.items;
  } catch (e) {
    message.error("加载快照明细失败: " + errMsg(e));
  } finally {
    detailLoading.value[id] = false;
  }
}

/**
 * One-click undo (§46 Dangerous): load the per-repo undo plan with live
 * safety checks, show the impact list in a second confirmation, then run
 * the reverse operations. Repos failing the check are skipped server-side.
 */
async function handleUndo(row: OperationLogSummary) {
  undoingId.value = row.id;
  try {
    let preview: UndoPreviewItem[];
    try {
      preview = await previewUndoOperation(row.id);
    } catch (e) {
      message.error("生成撤销预览失败: " + errMsg(e));
      return;
    }
    const runnable = preview.filter((p) => p.ok && !p.undone);
    if (runnable.length === 0) {
      message.warning("没有可安全撤销的仓库（全部已撤销或安全检查未通过）");
      return;
    }
    const lines = preview.map((p) => {
      const mark = p.undone ? "·" : p.ok ? "✓" : "✗";
      const text = p.undone
        ? "已撤销"
        : p.ok
          ? p.action
          : `${p.action}（跳过：${p.message}）`;
      return `${mark} ${p.repoName}：${text}`;
    });
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.error({
          title: "撤销确认（Dangerous）",
          content: `将撤销「${row.summary}」，对 ${runnable.length}/${preview.length} 个仓库执行反向操作；安全检查未通过的仓库会被跳过。\n\n${lines.join("\n")}`,
          positiveText: "执行撤销",
          negativeText: "取消",
          onPositiveClick: () => resolve(),
          onNegativeClick: () => reject(new Error("cancelled")),
          onClose: () => reject(new Error("cancelled")),
        });
      });
    } catch {
      return; // cancelled
    }

    const outcome = await undoOperation(row.id);
    const failed = outcome.results.filter((r) => !r.success);
    const okCount = outcome.results.length - failed.length;
    if (failed.length === 0) {
      message.success(`撤销完成（${okCount} 个仓库）`);
    } else {
      dialog.warning({
        title: `${okCount} 个仓库已撤销，${failed.length} 个被跳过 / 失败`,
        content: failed.map((f) => `${f.repoName}：${f.message}`).join("\n"),
      });
    }
    await reload();
  } catch (e) {
    message.error("撤销失败: " + errMsg(e));
  } finally {
    undoingId.value = null;
  }
}

// --- Main table columns ---
const columns = [
  {
    type: "expand" as const,
    renderExpand: (row: any) => {
      const items = details.value[row.id] ?? [];
      return h("div", { class: "items-wrap" }, [
        detailLoading.value[row.id]
          ? h("div", { style: "padding: 8px" }, "加载中...")
          : h("div", {}, [
              items.length === 0
                ? h("div", { style: "padding: 8px; color: #909399" }, "无明细")
                : h("div", { style: "padding: 4px 0" }, items.map((item: any) =>
                    h("div", { style: "display: flex; gap: 12px; padding: 4px 0; font-size: 12px; border-bottom: 1px solid #f0f0f0" }, [
                      h("span", { style: "font-weight: 500; min-width: 120px" }, repoName(item.repoPath)),
                      h("span", { style: "min-width: 100px" }, item.refName || "（分离 HEAD）"),
                      h("span", {}, [
                        h("code", {}, shortOid(item.beforeOid)),
                        h("span", { class: "arrow" }, "→"),
                        item.afterOid
                          ? h("code", {}, shortOid(item.afterOid))
                          : h("span", { class: "after-none" }, "—（未记录 / 已删除）"),
                      ]),
                      h("span", { style: "color: #909399" }, formatDetail(item.detail)),
                      item.undoneAt
                        ? h("span", { style: "color: #409eff; font-size: 12px" }, "已撤销")
                        : null,
                    ]),
                  )),
            ]),
      ]);
    },
  },
  { title: "时间", key: "createdAt", width: 170, render: (row: any) => formatDate(row.createdAt) },
  {
    title: "操作",
    key: "opType",
    width: 130,
    render: (row: any) => {
      const meta = opTypeMeta(row.opType);
      return h(NTag, { type: meta.tag, size: "small" }, { default: () => meta.label });
    },
  },
  { title: "摘要", key: "summary", minWidth: 220, ellipsis: { tooltip: true } },
  { title: "仓库数", key: "repoCount", width: 80, align: "center" as const },
  {
    title: "状态",
    key: "status",
    width: 100,
    render: (row: any) => {
      const s = statusOf(row as OperationLogSummary);
      return h(NTag, { type: s.type, size: "small" }, { default: () => s.label });
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 110,
    render: (row: any) =>
      h(
        NButton,
        {
          size: "small",
          type: "error",
          secondary: true,
          disabled: !!row.undoneAt,
          loading: undoingId.value === row.id,
          onClick: () => handleUndo(row as OperationLogSummary),
        },
        { default: () => "撤销" },
      ),
  },
];

onMounted(async () => {
  if (workspaceStore.workspaces.length === 0) {
    await workspaceStore.loadWorkspaces();
  }
  await reload();
});
</script>

<style scoped>
.operation-log-view {
  padding: 16px 24px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  height: 100%;
  box-sizing: border-box;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  flex-wrap: wrap;
}

.items-wrap {
  padding: 8px 16px;
  background: var(--gw-bg-hover);
}

.items-wrap code {
  font-family: var(--gw-font-mono);
}

.repo-cell {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}

.repo-name {
  font-weight: 500;
}

.repo-path {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.arrow {
  margin: 0 6px;
  color: var(--gw-text-dim);
}

.after-none {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.pager {
  display: flex;
  justify-content: flex-end;
}
</style>
