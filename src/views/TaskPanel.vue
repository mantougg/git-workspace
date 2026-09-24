<template>
  <n-drawer
    v-model:show="visible"
    placement="bottom"
    :height="panelHeight"
    :show-header="false"
    class="task-drawer"
  >
    <div class="drawer-resize" @mousedown="startResize"></div>
    <div class="drawer-title">
      <span class="drawer-title-text">命令流</span>
    </div>
    <div class="task-panel-content">
      <div class="task-panel-toolbar">
        <span class="task-count">
          {{ activeCount }} 个进行中 / {{ finishedCount }} 个已完成
        </span>
        <n-button size="small" text @click="handleClear">
          清除已完成
        </n-button>
      </div>

      <n-scrollbar class="task-scroll">
        <div v-if="timeline.length === 0" class="empty-tasks">
          <n-empty description="暂无命令记录" />
        </div>
        <div
          v-for="row in timeline"
          :key="row.key"
          class="event-row"
          :class="{
            'event-failed': row.status.type === 'failed',
            'event-cancelled': row.status.type === 'cancelled',
            'event-batch': row.isBatch,
          }"
        >
          <!-- 时间列 -->
          <span class="event-time">{{ row.time }}</span>
          <!-- 类型 badge -->
          <span class="task-type-badge" :class="taskTypeClass(row.taskType)">
            {{ taskTypeLabel(row.taskType) }}
          </span>
          <!-- repo 名（可点击跳转仓库视图） -->
          <span
            class="event-repo"
            :title="row.repoPath"
            @click="openRepo(row.repoPath)"
          >
            {{ row.repoName }}
          </span>
          <!-- 状态 / 耗时 -->
          <span class="event-status">
            <n-tag
              v-if="row.status.type === 'queued'"
              type="default"
              size="small"
            >
              排队中
            </n-tag>
            <n-tag
              v-else-if="row.status.type === 'running'"
              type="warning"
              size="small"
            >
              <n-spin :size="12" /> 执行中
            </n-tag>
            <n-tag
              v-else-if="row.status.type === 'success'"
              type="success"
              size="small"
            >
              成功
            </n-tag>
            <n-tag
              v-else-if="row.status.type === 'partialSuccess'"
              type="warning"
              size="small"
            >
              部分成功 {{ row.status.succeeded }}/{{
                row.status.succeeded + row.status.failed
              }}
            </n-tag>
            <n-tag
              v-else-if="row.status.type === 'failed'"
              type="error"
              size="small"
            >
              失败
            </n-tag>
            <n-tag
              v-else-if="row.status.type === 'cancelled'"
              type="default"
              size="small"
            >
              已取消
            </n-tag>
            <span v-if="row.durationText" class="event-duration">
              {{ row.durationText }}
            </span>
          </span>
          <!-- 行内操作 -->
          <span class="event-actions">
            <n-button
              v-if="row.isBatch && row.childCount > 0"
              size="small"
              text
              @click="toggleBatch(row.taskId)"
            >
              {{ expandedBatches.has(row.taskId) ? '收起' : `明细 ${row.childCount}` }}
            </n-button>
            <n-button
              v-if="row.task && row.task.status.type === 'queued'"
              size="small"
              text
              type="error"
              @click="handleCancel(row.taskId)"
            >
              取消
            </n-button>
            <n-button
              v-if="row.status.type === 'failed' && row.status.error"
              size="small"
              text
              @click="toggleError(row.seq)"
            >
              {{ expandedErrors.has(row.seq) ? '收起错误' : '展开错误' }}
            </n-button>
          </span>
          <!-- 失败错误详情 -->
          <div
            v-if="row.status.type === 'failed' && expandedErrors.has(row.seq)"
            class="event-error"
          >
            {{ row.status.error }}
            <!-- GF-08：认证/网络等分类的可行动引导（原因 + 建议操作 + 重试） -->
            <div v-if="row.guidance" class="event-guidance">
              <n-tag size="small" :type="guidanceTagType(row.guidance)">
                {{ guidanceLabel(row.guidance) }}
              </n-tag>
              <span class="guidance-reason">{{ row.guidance.reason }}</span>
              <span v-if="row.guidance.actions.length" class="guidance-actions">
                建议：{{ row.guidance.actions.join("；") }}
              </span>
              <n-button
                v-if="isRetryable(row)"
                size="tiny"
                text
                type="primary"
                @click="retryTask(row)"
              >
                重试
              </n-button>
            </div>
          </div>
          <!-- batch 子行 -->
          <div v-if="row.isBatch && expandedBatches.has(row.taskId)" class="batch-children">
            <div
              v-for="child in childrenOf(row.taskId)"
              :key="child.key"
              class="batch-child"
              :class="child.status.type"
            >
              <span class="child-time">{{ child.time }}</span>
              <span :class="['child-mark', child.status.type]">
                {{ childMark(child) }}
              </span>
              <span class="child-repo" @click="openRepo(child.repoPath)">
                {{ child.repoName }}
              </span>
              <span v-if="child.durationText" class="child-duration">
                {{ child.durationText }}
              </span>
              <span
                v-if="child.status.type === 'failed' && expandedErrors.has(child.seq)"
                class="child-error"
              >
                {{ child.status.error }}
                <span v-if="child.guidance" class="child-guidance">
                  {{ guidanceLabel(child.guidance) }}：{{ child.guidance.actions.join("；") }}
                </span>
              </span>
              <n-button
                v-else-if="child.status.type === 'failed' && child.status.error"
                size="tiny"
                text
                @click="toggleError(child.seq)"
              >
                展开错误
              </n-button>
            </div>
          </div>
        </div>
      </n-scrollbar>
    </div>
  </n-drawer>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { useTaskStore } from "@/stores/task";
import type { Task, TaskType, TaskStatus } from "@/types/task";
import { submitTasks } from "@/api/task";
import {
  classifyGitErrorText,
  GIT_ERROR_LABELS,
  type GitErrorCategoryCode,
  type GitErrorGuidance,
} from "@/utils/gitError";

const taskStore = useTaskStore();
const router = useRouter();const panelHeight = ref(420);
let resizeStartY = 0;
let resizeStartH = 0;

/** Start dragging the drawer height from its top edge. */
function startResize(e: MouseEvent) {
  e.preventDefault();
  resizeStartY = e.clientY;
  resizeStartH = panelHeight.value;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
}

function onResizeMove(e: MouseEvent) {
  const delta = e.clientY - resizeStartY; // drag up -> taller
  panelHeight.value = Math.max(
    240,
    Math.min(window.innerHeight * 0.85, resizeStartH + delta)
  );
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
}

const visible = computed({
  get: () => taskStore.panelVisible,
  set: (val) => { taskStore.panelVisible = val; },
});

const tasks = computed(() => taskStore.tasks);
const activeCount = computed(
  () =>
    tasks.value.filter(
      (t) => t.status.type === "queued" || t.status.type === "running"
    ).length
);
const finishedCount = computed(
  () =>
    tasks.value.filter(
      (t) =>
        t.status.type === "success" ||
        t.status.type === "failed" ||
        t.status.type === "cancelled" ||
        t.status.type === "partialSuccess"
    ).length
);

// ---------------------------------------------------------------------------
// TM-08：命令流时间线
// ---------------------------------------------------------------------------

interface TimelineRow {
  key: string;
  seq: number;
  taskId: string;
  time: string;
  repoPath: string;
  repoName: string;
  taskType: TaskType;
  status: TaskStatus;
  durationText: string;
  isBatch: boolean;
  childCount: number;
  /** GF-08：失败错误的可行动分类（认证/网络/锁/脏工作区/被拒绝）。 */
  guidance: GitErrorGuidance | null;
  /** 关联的当前任务快照（queued 取消按钮用；刷新后无对应任务则为 undefined）。 */
  task?: Task;
}

/** GF-08：失败错误文本 → 可行动引导（纯函数镜像后端 classify_git_error）。 */
function guidanceOf(status: TaskStatus): GitErrorGuidance | null {
  if (status.type !== "failed") return null;
  return classifyGitErrorText(status.error);
}

/** GF-08：分类 chip 文案与配色。 */
function guidanceLabel(guidance: GitErrorGuidance): string {
  const category = guidance.category as GitErrorCategoryCode | undefined;
  return category && category in GIT_ERROR_LABELS
    ? GIT_ERROR_LABELS[category]
    : "Git 失败";
}

function guidanceTagType(guidance: GitErrorGuidance): "error" | "warning" | "default" {
  return guidance.category === "authentication" ? "error" : "warning";
}

/** GF-08：可重试的任务类型（与后端 worker 的 retryable 集合一致）。 */
const RETRYABLE_TASK_TYPES = new Set(["fetch", "pull", "push", "clone"]);

function isRetryable(row: { taskType: TaskType }): boolean {
  return RETRYABLE_TASK_TYPES.has(row.taskType.type);
}

/** GF-08：失败行「重试」——按同类型同仓库重新入队（修好凭据后一键再来）。 */
async function retryTask(row: { repoPath: string; repoName: string; taskType: TaskType }) {
  if (!isRetryable(row)) return;
  try {
    await submitTasks([
      { taskType: row.taskType, repoPath: row.repoPath, repoName: row.repoName },
    ]);
    taskStore.showPanel();
  } catch (e) {
    console.error("retry task failed:", e);
  }
}

function hhmmss(at: number): string {
  const d = new Date(at);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function durationText(ms?: number): string {
  if (ms === undefined) return "";
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

/**
 * 事件流行：事件按 seq 升序；batch 父行（id ∈ children 的 batchId 集合）
 * 插入到其**首个 child 事件**的时间点，子行收在父行「明细」里。
 */
const timeline = computed<TimelineRow[]>(() => {
  const events = taskStore.events;
  const batchIds = new Set(
    events
      .map((e) => e.batchId)
      .filter((b): b is string => !!b)
  );
  const rows: TimelineRow[] = [];
  const firstChildSeqByBatch = new Map<string, number>();
  for (const e of events) {
    if (e.batchId && !firstChildSeqByBatch.has(e.batchId)) {
      firstChildSeqByBatch.set(e.batchId, e.seq);
    }
  }
  for (const e of events) {
    const isBatch = !e.batchId && batchIds.has(e.taskId);
    const insertAt = isBatch
      ? (firstChildSeqByBatch.get(e.taskId) ?? e.seq) - 0.5
      : e.seq;
    rows.push({
      key: `e${e.seq}`,
      seq: e.seq,
      taskId: e.taskId,
      time: hhmmss(e.at),
      repoPath: e.repoPath,
      repoName: e.repoName,
      taskType: e.taskType,
      status: e.status,
      durationText: durationText(e.durationMs),
      isBatch,
      childCount: isBatch
        ? events.filter((c) => c.batchId === e.taskId).length
        : 0,
      guidance: guidanceOf(e.status),
      task: tasks.value.find((t) => t.id === e.taskId),
      // insertAt 参与排序：batch 父行排在其首个子事件之前
      ...(isBatch ? {} : {}),
    });
    if (isBatch) {
      // 用 -0.5 偏移保证父行先于同刻子事件；seq 为整数，直接减 0.5 即可
      rows[rows.length - 1].seq = insertAt;
    }
  }
  // batch 行按首个子事件时间插入；其余按事件序
  rows.sort((a, b) => a.seq - b.seq);
  return rows;
});

function childrenOf(batchTaskId: string): TimelineRow[] {
  return taskStore.events
    .filter((e) => e.batchId === batchTaskId)
    .map((e) => ({
      key: `c${e.seq}`,
      seq: e.seq,
      taskId: e.taskId,
      time: hhmmss(e.at),
      repoPath: e.repoPath,
      repoName: e.repoName,
      taskType: e.taskType,
      status: e.status,
      durationText: durationText(e.durationMs),
      isBatch: false,
      childCount: 0,
      guidance: guidanceOf(e.status),
    }));
}

const expandedBatches = ref<Set<string>>(new Set());
const expandedErrors = ref<Set<number>>(new Set());

function toggleBatch(taskId: string) {
  const next = new Set(expandedBatches.value);
  if (next.has(taskId)) {
    next.delete(taskId);
  } else {
    next.add(taskId);
  }
  expandedBatches.value = next;
}

function toggleError(seq: number) {
  const next = new Set(expandedErrors.value);
  if (next.has(seq)) {
    next.delete(seq);
  } else {
    next.add(seq);
  }
  expandedErrors.value = next;
}

function childMark(row: { status: TaskStatus }): string {
  switch (row.status.type) {
    case "success":
      return "✓";
    case "failed":
      return "✗";
    case "cancelled":
      return "⊘";
    default:
      return "…";
  }
}

function taskTypeLabel(taskType: TaskType): string {
  switch (taskType.type) {
    case "fetch":
      return "Fetch";
    case "pull":
      return "Pull";
    case "push":
      return "Push";
    case "stageFiles":
      return "暂存";
    case "restoreFiles":
      return "还原工作区";
    case "commit":
      return "Commit";
    case "conflictApply":
      return "Conflict Apply";
    case "branchOp":
      return "分支操作";
    case "clone":
      return "Clone";
    case "shellCommand":
      return "Shell";
    case "runtime":
      return "Runtime";
    case "runtimeUpdateConfig":
      return "Runtime 配置更新";
    case "nodeInstall":
      return "Node Install";
  }
}

function taskTypeClass(taskType: TaskType): string {
  return `task-type-${taskType.type}`;
}

/** 点击行跳转仓库视图（RepositoryList 的 query.repo 既有模式）。 */
function openRepo(repoPath: string) {
  if (!repoPath) return;
  router.push({ name: "repositories", query: { repo: repoPath } });
}

async function handleCancel(taskId: string) {
  await taskStore.cancelTask(taskId);
}

async function handleClear() {
  await taskStore.clearFinished();
}
</script>

<style scoped>
.task-drawer {
  /* Compact header/body spacing for the bottom task drawer. */
}

:deep(.n-drawer-body-content-wrapper) {
  padding: 0 12px 8px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.drawer-resize {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 8px;
  cursor: ns-resize;
  z-index: 10;
}

.drawer-resize:hover {
  background: var(--gw-accent);
}

.drawer-title {
  display: flex;
  align-items: center;
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  flex-shrink: 0;
}

.drawer-title-text {
  font-size: 13px;
  font-weight: 600;
  color: var(--gw-text);
}

.task-panel-content {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding-top: 8px;
}

.task-scroll {
  flex: 1;
  min-height: 0;
}

.task-panel-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 0 8px;
  border-bottom: 1px solid var(--gw-border);
}

.task-count {
  font-size: 13px;
  color: var(--gw-text-dim);
}

/* ---- 事件流行（TM-08） ---- */

.event-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 4px 8px;
  border-bottom: 1px solid var(--gw-border);
  flex-wrap: wrap;
  font-size: 13px;
}

.event-row.event-failed {
  background: color-mix(in srgb, var(--gw-danger) 10%, transparent);
}

.event-row.event-cancelled {
  opacity: 0.75;
}

.event-row.event-batch {
  font-weight: 600;
}

.event-time {
  font-family: var(--gw-font-mono);
  font-size: 11px;
  color: var(--gw-text-dim);
  flex-shrink: 0;
  min-width: 56px;
}

.task-type-badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 3px;
  font-weight: 600;
  flex-shrink: 0;
}

.task-type-fetch {
  background: var(--gw-bg-hover);
  color: var(--gw-accent);
}

.task-type-pull {
  background: var(--gw-bg-hover);
  color: var(--gw-success);
}

.task-type-push {
  background: var(--gw-bg-hover);
  color: var(--gw-warning);
}

.task-type-commit {
  background: var(--gw-bg-hover);
  color: var(--gw-danger);
}

.task-type-runtime,
.task-type-nodeInstall {
  background: var(--gw-bg-hover);
  color: var(--gw-accent);
}

.event-repo {
  font-weight: 500;
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
}

.event-repo:hover {
  color: var(--gw-accent);
  text-decoration: underline;
}

.event-status {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  margin-left: auto;
}

.event-duration {
  font-family: var(--gw-font-mono);
  font-size: 11px;
  color: var(--gw-text-dim);
}

.event-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.event-error {
  width: 100%;
  font-size: 12px;
  font-family: var(--gw-font-mono);
  color: var(--gw-danger);
  padding: 4px 6px;
  margin-top: 2px;
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-sm);
  white-space: pre-wrap;
  word-break: break-all;
}

/* GF-08：可行动引导块（分类 chip + 原因 + 建议操作 + 重试） */
.event-guidance {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-1);
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px dashed var(--gw-border);
  font-family: inherit;
  color: var(--gw-text);
}

.guidance-reason {
  font-size: 12px;
}

.guidance-actions {
  font-size: 12px;
  color: var(--gw-text-dim);
  word-break: break-all;
}

.batch-children {
  width: 100%;
  margin-top: 4px;
  border-top: 1px dashed var(--gw-border);
  padding-top: 4px;
}

.batch-child {
  display: flex;
  align-items: baseline;
  gap: var(--gw-space-2);
  font-size: 12px;
  font-weight: 400;
  padding: 1px 0;
}

.child-time {
  font-family: var(--gw-font-mono);
  font-size: 11px;
  color: var(--gw-text-dim);
  min-width: 56px;
}

.child-mark {
  width: 14px;
  text-align: center;
  flex-shrink: 0;
}

.child-mark.success {
  color: var(--gw-success);
}

.child-mark.failed {
  color: var(--gw-danger);
}

.child-mark.cancelled {
  color: var(--gw-text-dim);
}

.child-repo {
  cursor: pointer;
  border-radius: var(--gw-radius-sm);
}

.child-repo:hover {
  color: var(--gw-accent);
  text-decoration: underline;
}

.child-duration {
  font-family: var(--gw-font-mono);
  font-size: 11px;
  color: var(--gw-text-dim);
  margin-left: auto;
}

.child-error {
  color: var(--gw-danger);
  word-break: break-all;
}

/* GF-08：批量子行的紧凑引导（分类标签 + 建议操作） */
.child-guidance {
  display: block;
  margin-top: 2px;
  color: var(--gw-text-dim);
  word-break: break-all;
}

.empty-tasks {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 200px;
}
</style>
