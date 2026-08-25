<template>
  <div class="operation-log-view">
    <!-- Top toolbar: filters (workspace / op type / repo / date range) -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="filterWorkspaceId"
          placeholder="全部工作区"
          clearable
          style="width: 160px"
          @change="reload"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-select
          v-model="filterOpType"
          placeholder="全部操作类型"
          clearable
          style="width: 170px"
          @change="reload"
        >
          <el-option
            v-for="o in opTypeOptions"
            :key="o.value"
            :label="o.label"
            :value="o.value"
          />
        </el-select>
        <el-input
          v-model="filterRepo"
          placeholder="按仓库路径筛选"
          clearable
          style="width: 200px"
          @change="reload"
          @clear="reload"
        />
        <el-date-picker
          v-model="dateRange"
          type="daterange"
          value-format="YYYY-MM-DD"
          start-placeholder="开始日期"
          end-placeholder="结束日期"
          style="width: 240px"
          @change="reload"
        />
        <el-button :loading="loading" @click="reload">
          <el-icon><Refresh /></el-icon>
          刷新
        </el-button>
      </div>
    </div>

    <!-- Log table; expanding a row lazy-loads its per-repo ref snapshots -->
    <el-table
      :data="logs"
      v-loading="loading"
      size="small"
      row-key="id"
      @expand-change="onExpand"
    >
      <el-table-column type="expand">
        <template #default="{ row }">
          <div class="items-wrap" v-loading="detailLoading[row.id]">
            <el-table :data="details[row.id] ?? []" size="small">
              <el-table-column label="仓库" min-width="200">
                <template #default="{ row: item }">
                  <div class="repo-cell">
                    <span class="repo-name">{{ repoName(item.repoPath) }}</span>
                    <span class="repo-path">{{ item.repoPath }}</span>
                  </div>
                </template>
              </el-table-column>
              <el-table-column label="Ref" width="140">
                <template #default="{ row: item }">
                  {{ item.refName || "（分离 HEAD）" }}
                </template>
              </el-table-column>
              <el-table-column label="Before → After" min-width="180">
                <template #default="{ row: item }">
                  <code>{{ shortOid(item.beforeOid) }}</code>
                  <span class="arrow">→</span>
                  <code v-if="item.afterOid">{{ shortOid(item.afterOid) }}</code>
                  <span v-else class="after-none">—（未记录 / 已删除）</span>
                </template>
              </el-table-column>
              <el-table-column label="备注" min-width="140">
                <template #default="{ row: item }">
                  {{ formatDetail(item.detail) }}
                </template>
              </el-table-column>
              <el-table-column label="状态" width="90">
                <template #default="{ row: item }">
                  <el-tag v-if="item.undoneAt" type="info" size="small">已撤销</el-tag>
                  <span v-else>—</span>
                </template>
              </el-table-column>
            </el-table>
          </div>
        </template>
      </el-table-column>
      <el-table-column label="时间" width="170">
        <template #default="{ row }">{{ formatDate(row.createdAt) }}</template>
      </el-table-column>
      <el-table-column label="操作" width="130">
        <template #default="{ row }">
          <el-tag :type="opTypeMeta(row.opType).tag" size="small">
            {{ opTypeMeta(row.opType).label }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column prop="summary" label="摘要" min-width="220" show-overflow-tooltip />
      <el-table-column prop="repoCount" label="仓库数" width="80" align="center" />
      <el-table-column label="状态" width="100">
        <template #default="{ row }">
          <el-tag :type="statusOf(row as OperationLogSummary).type" size="small">{{ statusOf(row as OperationLogSummary).label }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="110">
        <template #default="{ row }">
          <el-button
            size="small"
            type="danger"
            plain
            :disabled="!!row.undoneAt"
            :loading="undoingId === row.id"
            @click="handleUndo(row as OperationLogSummary)"
          >
            撤销
          </el-button>
        </template>
      </el-table-column>
    </el-table>

    <div class="pager">
      <el-pagination
        v-model:current-page="page"
        layout="total, prev, pager, next"
        :total="total"
        :page-size="pageSize"
        @current-change="reload"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { Back, Refresh } from "@element-plus/icons-vue";
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

const router = useRouter();
const workspaceStore = useWorkspaceStore();

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

const undoingId = ref<number | null>(null);

/** op_type → display meta (all four logged kinds are reversible). */
const OP_TYPE_META: Record<string, { label: string; tag: "warning" | "danger" | "info" }> = {
  checkout_all: { label: "批量检出", tag: "warning" },
  delete_branch_all: { label: "批量删除分支", tag: "danger" },
  reset: { label: "Reset", tag: "danger" },
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

function goBack() {
  router.back();
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
    ElMessage.error("加载操作日志失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

/** Lazy-load the per-repo ref snapshots when a row is expanded. */
async function onExpand(row: OperationLogSummary, expanded: any) {
  if (!expanded.includes(row) || details.value[row.id]) return;
  detailLoading.value[row.id] = true;
  try {
    const d = await getOperationLogDetail(row.id);
    details.value[row.id] = d.items;
  } catch (e) {
    ElMessage.error("加载快照明细失败: " + errMsg(e));
  } finally {
    detailLoading.value[row.id] = false;
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
      ElMessage.error("生成撤销预览失败: " + errMsg(e));
      return;
    }
    const runnable = preview.filter((p) => p.ok && !p.undone);
    if (runnable.length === 0) {
      ElMessage.warning("没有可安全撤销的仓库（全部已撤销或安全检查未通过）");
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
      await ElMessageBox.confirm(
        `将撤销「${row.summary}」，对 ${runnable.length}/${preview.length} 个仓库执行反向操作；安全检查未通过的仓库会被跳过。\n\n${lines.join("\n")}`,
        "撤销确认（Dangerous）",
        {
          confirmButtonText: "执行撤销",
          cancelButtonText: "取消",
          type: "error",
          confirmButtonClass: "el-button--danger",
        },
      );
    } catch {
      return; // cancelled
    }

    const outcome = await undoOperation(row.id);
    const failed = outcome.results.filter((r) => !r.success);
    const okCount = outcome.results.length - failed.length;
    if (failed.length === 0) {
      ElMessage.success(`撤销完成（${okCount} 个仓库）`);
    } else {
      ElMessageBox.alert(
        failed.map((f) => `${f.repoName}：${f.message}`).join("\n"),
        `${okCount} 个仓库已撤销，${failed.length} 个被跳过 / 失败`,
        { type: "warning" },
      );
    }
    await reload();
  } catch (e) {
    ElMessage.error("撤销失败: " + errMsg(e));
  } finally {
    undoingId.value = null;
  }
}

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
  gap: 12px;
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
  gap: 8px;
  flex-wrap: wrap;
}

.items-wrap {
  padding: 8px 16px;
  background: var(--el-fill-color-light);
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
  color: var(--el-text-color-secondary);
}

.arrow {
  margin: 0 6px;
  color: var(--el-text-color-secondary);
}

.after-none {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

.pager {
  display: flex;
  justify-content: flex-end;
}
</style>
