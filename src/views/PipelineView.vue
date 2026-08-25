<template>
  <div class="pipeline-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 180px"
          @change="onWorkspaceChange"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-input
          v-model="pipeline.name"
          placeholder="Pipeline 名称"
          style="width: 240px"
        />
      </div>
      <div class="toolbar-right">
        <el-select
          v-model="selectedTemplateId"
          placeholder="加载模板…"
          style="width: 200px"
          clearable
          @change="loadTemplate"
        >
          <el-option
            v-for="t in templates"
            :key="t.id"
            :label="t.name"
            :value="t.id"
          />
        </el-select>
        <el-button size="small" @click="loadSample">示例模板</el-button>
        <el-button size="small" type="primary" plain @click="saveTemplate">
          保存模板
        </el-button>
        <el-button
          v-if="pipeline.id"
          size="small"
          type="danger"
          plain
          @click="removeTemplate"
        >
          删除模板
        </el-button>
      </div>
    </div>

    <div class="main-area">
      <!-- Left: step editor -->
      <div class="editor-panel">
        <div class="panel-title">步骤编排（T-23）</div>
        <el-scrollbar class="editor-scroll">
          <div
            v-for="(step, i) in pipeline.steps"
            :key="step.id"
            class="step-card"
          >
            <div class="step-head">
              <span class="step-order">{{ i + 1 }}</span>
              <el-input v-model="step.name" size="small" class="step-name" />
              <el-button-group class="step-moves">
                <el-button
                  size="small"
                  text
                  :disabled="i === 0"
                  @click="moveStep(i, -1)"
                >
                  ↑
                </el-button>
                <el-button
                  size="small"
                  text
                  :disabled="i === pipeline.steps.length - 1"
                  @click="moveStep(i, 1)"
                >
                  ↓
                </el-button>
              </el-button-group>
              <el-button
                size="small"
                text
                type="danger"
                @click="removeStep(i)"
              >
                删除
              </el-button>
            </div>
            <div class="step-body">
              <div class="step-row">
                <span class="step-label">类型</span>
                <el-select
                  :model-value="step.kind.type"
                  size="small"
                  style="width: 150px"
                  @change="(v: string) => changeKind(step, v)"
                >
                  <el-option label="Fetch" value="fetch" />
                  <el-option label="Check Status" value="checkStatus" />
                  <el-option label="Pull" value="pull" />
                  <el-option label="Build (Shell)" value="build" />
                  <el-option label="Test (Shell)" value="test" />
                  <el-option label="Report (汇聚)" value="report" />
                </el-select>
              </div>
              <div
                v-if="step.kind.type === 'build' || step.kind.type === 'test'"
                class="step-row"
              >
                <span class="step-label">命令</span>
                <el-input
                  :model-value="step.kind.command"
                  size="small"
                  placeholder="如 cargo build"
                  @update:model-value="(v: string) => setCommand(step, v)"
                />
              </div>
              <div class="step-row" v-if="step.kind.type !== 'report'">
                <span class="step-label">条件</span>
                <el-select
                  :model-value="step.condition?.type ?? ''"
                  size="small"
                  style="width: 150px"
                  clearable
                  placeholder="无条件"
                  @change="(v: string) => setCondition(step, v)"
                >
                  <el-option label="仅干净仓库" value="repoClean" />
                </el-select>
                <span class="step-label">重试</span>
                <el-input-number
                  v-model="step.retries"
                  size="small"
                  :min="0"
                  :max="3"
                  style="width: 80px"
                />
              </div>
              <div
                class="step-row"
                v-if="step.kind.type === 'build' || step.kind.type === 'test'"
              >
                <span class="step-label">超时(s)</span>
                <el-input-number
                  :model-value="step.timeoutSecs ?? undefined"
                  size="small"
                  :min="1"
                  :max="3600"
                  placeholder="600"
                  style="width: 110px"
                  @update:model-value="(v: number | undefined) => setTimeoutSecs(step, v)"
                />
              </div>
              <div class="step-row" v-if="step.kind.type !== 'report'">
                <span class="step-label">依赖</span>
                <el-select
                  v-model="step.dependsOn"
                  size="small"
                  multiple
                  collapse-tags
                  placeholder="默认依赖上一步"
                  style="flex: 1"
                >
                  <el-option
                    v-for="opt in upstreamOptions(step)"
                    :key="opt.id"
                    :label="opt.name"
                    :value="opt.id"
                  />
                </el-select>
              </div>
            </div>
          </div>
          <el-button class="add-step" size="small" @click="addStep">
            + 添加步骤
          </el-button>
        </el-scrollbar>
      </div>

      <!-- Right: graph + run + report -->
      <div class="stage-panel">
        <!-- Run controls -->
        <div class="run-bar">
          <el-select
            v-model="selectedRepoPaths"
            multiple
            collapse-tags
            :max-collapse-tags="3"
            placeholder="选择仓库（可多选）"
            style="flex: 1; min-width: 220px"
          >
            <el-option
              v-for="r in repoOptions"
              :key="r.repoPath"
              :label="r.repoName"
              :value="r.repoPath"
            />
          </el-select>
          <el-button size="small" text @click="selectAllRepos">全选</el-button>
          <el-select v-model="onFailure" style="width: 170px" size="small">
            <el-option label="失败：继续独立分支" value="continue" />
            <el-option label="失败：Fail-Fast" value="failFast" />
          </el-select>
          <el-button
            type="primary"
            :loading="runStarting"
            :disabled="!canRun"
            @click="run"
          >
            运行 Pipeline
          </el-button>
          <el-button
            v-if="runActive"
            type="danger"
            plain
            @click="cancelRun"
          >
            取消运行
          </el-button>
        </div>

        <!-- Graph visualization -->
        <div class="graph-box">
          <div class="panel-title">
            流程图
            <span v-if="report" class="run-summary">
              运行状态：
              <el-tag :type="statusTagType(report.status)" size="small">
                {{ statusLabel(report.status) }}
              </el-tag>
              成功 {{ report.succeeded }} / 失败 {{ report.failed }} / 跳过
              {{ report.skipped }} / 取消 {{ report.cancelled }} · 共
              {{ report.total }} 节点
              <span v-if="report.durationMs != null">
                · {{ formatDuration(report.durationMs) }}
              </span>
            </span>
          </div>
          <div ref="graphRef" class="graph-canvas">
            <svg class="graph-edges">
              <path
                v-for="(e, i) in edgePaths"
                :key="i"
                :d="e"
                class="edge-path"
              />
            </svg>
            <div
              v-for="(layer, li) in graphLayers"
              :key="li"
              class="graph-layer"
            >
              <div
                v-for="step in layer"
                :key="step.id"
                :data-step-id="step.id"
                class="gnode"
                :class="['node-' + stepStatus(step.id), { 'is-report': step.kind.type === 'report' }]"
              >
                <div class="gnode-name">{{ step.name }}</div>
                <div class="gnode-kind">{{ kindLabel(step) }}</div>
                <div v-if="step.condition" class="gnode-cond">条件: 干净仓库</div>
              </div>
            </div>
            <div v-if="pipeline.steps.length === 0" class="graph-empty">
              添加步骤以编排 Pipeline
            </div>
          </div>
        </div>

        <!-- Execution report -->
        <div v-if="report" class="report-box">
          <div class="panel-title">执行报告</div>
          <el-scrollbar class="report-scroll">
            <div
              v-for="step in report.steps"
              :key="step.stepId"
              class="report-step"
            >
              <div class="report-step-head" @click="toggleStepDetail(step.stepId)">
                <el-tag :type="statusTagType(step.status)" size="small">
                  {{ statusLabel(step.status) }}
                </el-tag>
                <span class="report-step-name">{{ step.name }}</span>
                <span class="report-step-kind">{{ step.kind }}</span>
                <span class="report-step-stats">
                  {{ step.succeeded }}/{{ step.total }} 成功
                  <template v-if="step.failed">· {{ step.failed }} 失败</template>
                  <template v-if="step.skipped">· {{ step.skipped }} 跳过</template>
                  <template v-if="step.cancelled">· {{ step.cancelled }} 取消</template>
                </span>
                <span v-if="step.durationMs != null" class="report-step-dur">
                  {{ formatDuration(step.durationMs) }}
                </span>
                <span class="report-step-toggle">
                  {{ expandedSteps.has(step.stepId) ? "收起" : "明细" }}
                </span>
              </div>
              <div
                v-if="expandedSteps.has(step.stepId) && step.items.length > 0"
                class="report-items"
              >
                <div
                  v-for="item in step.items"
                  :key="item.taskId"
                  class="report-item"
                >
                  <span class="item-mark" :class="'mark-' + item.status">
                    {{ itemMark(item.status) }}
                  </span>
                  <span class="item-repo" :title="item.repoPath">
                    {{ item.repoName }}
                  </span>
                  <span class="item-status">{{ statusLabel(item.status) }}</span>
                  <span v-if="item.attempts > 1" class="item-attempts">
                    {{ item.attempts }} 次尝试
                  </span>
                  <span v-if="item.durationMs != null" class="item-dur">
                    {{ formatDuration(item.durationMs) }}
                  </span>
                  <span v-if="item.message" class="item-msg" :title="item.message">
                    {{ item.message }}
                  </span>
                  <pre v-if="item.output" class="item-output">{{ item.output }}</pre>
                </div>
              </div>
            </div>
          </el-scrollbar>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { Back } from "@element-plus/icons-vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import * as pipelineApi from "@/api/pipeline";
import type {
  FailurePolicy,
  Pipeline,
  PipelineRunReport,
  PipelineStep,
  RepoSelection,
  StepKind,
} from "@/types/pipeline";
import type { TaskProgress } from "@/types/task";

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const templates = ref<Pipeline[]>([]);
const selectedTemplateId = ref<string | null>(null);
const selectedWorkspaceId = ref<number | null>(null);
const selectedRepoPaths = ref<string[]>([]);
const onFailure = ref<FailurePolicy>("continue");

let stepCounter = 0;
function newStepId(): string {
  stepCounter += 1;
  return `step-${Date.now()}-${stepCounter}`;
}

function emptyPipeline(): Pipeline {
  return {
    id: "",
    name: "",
    description: "",
    steps: [],
    createdAt: "",
    updatedAt: "",
  };
}

const pipeline = ref<Pipeline>(emptyPipeline());

const runId = ref<string | null>(null);
const report = ref<PipelineRunReport | null>(null);
const runStarting = ref(false);
const expandedSteps = ref<Set<string>>(new Set());

const runActive = computed(() => {
  if (!report.value) return runStarting.value;
  return report.value.status === "running" || report.value.status === "pending";
});

const repoOptions = computed<RepoSelection[]>(() =>
  repoStore.repositories.map((r) => ({
    repoPath: r.repository.path,
    repoName: r.repository.name,
  })),
);

const canRun = computed(
  () =>
    !runActive.value &&
    pipeline.value.name.trim().length > 0 &&
    pipeline.value.steps.length > 0 &&
    selectedRepoPaths.value.length > 0,
);

// ---------------------------------------------------------------------------
// Lifecycle + data loading
// ---------------------------------------------------------------------------

let unlisten: UnlistenFn | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    await repoStore.loadRepositories(selectedWorkspaceId.value);
  }
  await refreshTemplates();

  // Live run updates: task_progress is already aggregated per node (T-24);
  // throttle report refreshes to keep IPC cheap on large runs.
  unlisten = await listen<TaskProgress>("task_progress", () => {
    if (!runId.value) return;
    if (refreshTimer) return;
    refreshTimer = setTimeout(() => {
      refreshTimer = null;
      refreshReport();
    }, 800);
  });

  window.addEventListener("resize", redrawEdges);
});

onUnmounted(() => {
  if (unlisten) unlisten();
  if (refreshTimer) clearTimeout(refreshTimer);
  window.removeEventListener("resize", redrawEdges);
});

function onWorkspaceChange(id: number) {
  const ws = workspaceStore.workspaces.find((w) => w.id === id);
  if (ws) workspaceStore.selectWorkspace(ws);
  selectedRepoPaths.value = [];
  repoStore.loadRepositories(id);
}

async function refreshTemplates() {
  try {
    templates.value = await pipelineApi.listPipelineTemplates();
  } catch (e) {
    console.error("Failed to load pipeline templates:", e);
  }
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

function loadTemplate(id: string | null) {
  const t = templates.value.find((x) => x.id === id);
  if (!t) return;
  pipeline.value = JSON.parse(JSON.stringify(t));
  ElMessage.success(`已加载模板「${t.name}」`);
}

async function loadSample() {
  try {
    const sample = await pipelineApi.getSamplePipeline();
    sample.id = pipeline.value.id; // keep editing the same template if loaded
    pipeline.value = sample;
    selectedTemplateId.value = null;
    ElMessage.success("已载入内置示例流（保存后可复用）");
  } catch (e) {
    ElMessage.error(`加载示例失败: ${e}`);
  }
}

async function saveTemplate() {
  try {
    const saved = await pipelineApi.savePipelineTemplate(pipeline.value);
    pipeline.value = JSON.parse(JSON.stringify(saved));
    selectedTemplateId.value = saved.id;
    await refreshTemplates();
    ElMessage.success("模板已保存");
  } catch (e) {
    ElMessage.error(`保存失败: ${e}`);
  }
}

async function removeTemplate() {
  if (!pipeline.value.id) return;
  try {
    await pipelineApi.deletePipelineTemplate(pipeline.value.id);
    pipeline.value = emptyPipeline();
    selectedTemplateId.value = null;
    await refreshTemplates();
    ElMessage.success("模板已删除");
  } catch (e) {
    ElMessage.error(`删除失败: ${e}`);
  }
}

// ---------------------------------------------------------------------------
// Step editor
// ---------------------------------------------------------------------------

function addStep() {
  pipeline.value.steps.push({
    id: newStepId(),
    name: `步骤 ${pipeline.value.steps.length + 1}`,
    kind: { type: "fetch" },
    dependsOn: [],
    condition: null,
    retries: 0,
    timeoutSecs: null,
  });
}

function removeStep(i: number) {
  const removed = pipeline.value.steps[i];
  pipeline.value.steps.splice(i, 1);
  // Drop dangling dependsOn references to the removed step.
  for (const s of pipeline.value.steps) {
    s.dependsOn = (s.dependsOn ?? []).filter((d) => d !== removed.id);
  }
}

function moveStep(i: number, dir: number) {
  const j = i + dir;
  if (j < 0 || j >= pipeline.value.steps.length) return;
  const steps = pipeline.value.steps;
  [steps[i], steps[j]] = [steps[j], steps[i]];
}

function changeKind(step: PipelineStep, v: string) {
  const kinds: Record<string, StepKind> = {
    fetch: { type: "fetch" },
    checkStatus: { type: "checkStatus" },
    pull: { type: "pull" },
    build: { type: "build", command: "" },
    test: { type: "test", command: "" },
    report: { type: "report" },
  };
  step.kind = kinds[v] ?? { type: "fetch" };
  if (step.kind.type === "report") {
    step.condition = null;
  }
}

function setCommand(step: PipelineStep, v: string) {
  if (step.kind.type === "build" || step.kind.type === "test") {
    step.kind.command = v;
  }
}

function setCondition(step: PipelineStep, v: string) {
  step.condition = v === "repoClean" ? { type: "repoClean" } : null;
}

function setTimeoutSecs(step: PipelineStep, v: number | undefined) {
  step.timeoutSecs = v ?? null;
}

function upstreamOptions(step: PipelineStep): PipelineStep[] {
  return pipeline.value.steps.filter(
    (s) => s.id !== step.id && s.kind.type !== "report",
  );
}

// ---------------------------------------------------------------------------
// Graph visualization (layered by topological depth, SVG edges)
// ---------------------------------------------------------------------------

const graphRef = ref<HTMLElement | null>(null);
const edgePaths = ref<string[]>([]);

/** Resolved upstream ids: explicit dependsOn, else previous executable step. */
function upstreamsOf(step: PipelineStep): string[] {
  if (step.dependsOn && step.dependsOn.length > 0) return step.dependsOn;
  const idx = pipeline.value.steps.findIndex((s) => s.id === step.id);
  for (let i = idx - 1; i >= 0; i--) {
    if (pipeline.value.steps[i].kind.type !== "report") {
      return [pipeline.value.steps[i].id];
    }
  }
  return [];
}

const graphLayers = computed<PipelineStep[][]>(() => {
  const depths = new Map<string, number>();
  const depthOf = (step: PipelineStep, seen: Set<string>): number => {
    const cached = depths.get(step.id);
    if (cached !== undefined) return cached;
    if (seen.has(step.id)) return 0; // cycle guard
    seen.add(step.id);
    const ups = upstreamsOf(step)
      .map((u) => pipeline.value.steps.find((s) => s.id === u))
      .filter((s): s is PipelineStep => !!s);
    const d =
      ups.length === 0 ? 0 : 1 + Math.max(...ups.map((u) => depthOf(u, seen)));
    depths.set(step.id, d);
    return d;
  };
  for (const s of pipeline.value.steps) depthOf(s, new Set());

  const layers: PipelineStep[][] = [];
  for (const s of pipeline.value.steps) {
    const d = depths.get(s.id) ?? 0;
    while (layers.length <= d) layers.push([]);
    layers[d].push(s);
  }
  return layers;
});

function redrawEdges() {
  nextTick(() => {
    const container = graphRef.value;
    if (!container) {
      edgePaths.value = [];
      return;
    }
    const cRect = container.getBoundingClientRect();
    const paths: string[] = [];
    for (const step of pipeline.value.steps) {
      const toEl = container.querySelector(
        `[data-step-id="${step.id}"]`,
      ) as HTMLElement | null;
      if (!toEl) continue;
      const to = toEl.getBoundingClientRect();
      for (const upId of upstreamsOf(step)) {
        const fromEl = container.querySelector(
          `[data-step-id="${upId}"]`,
        ) as HTMLElement | null;
        if (!fromEl) continue;
        const from = fromEl.getBoundingClientRect();
        const x1 = from.right - cRect.left;
        const y1 = from.top + from.height / 2 - cRect.top;
        const x2 = to.left - cRect.left;
        const y2 = to.top + to.height / 2 - cRect.top;
        const mx = (x1 + x2) / 2;
        paths.push(`M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`);
      }
    }
    edgePaths.value = paths;
  });
}

watch(
  () => pipeline.value.steps,
  () => redrawEdges(),
  { deep: true },
);

function stepStatus(stepId: string): string {
  if (!report.value) return "idle";
  const s = report.value.steps.find((x) => x.stepId === stepId);
  return s?.status ?? "idle";
}

// ---------------------------------------------------------------------------
// Run + report
// ---------------------------------------------------------------------------

function selectAllRepos() {
  selectedRepoPaths.value = repoOptions.value.map((r) => r.repoPath);
}

async function run() {
  const repos = repoOptions.value.filter((r) =>
    selectedRepoPaths.value.includes(r.repoPath),
  );
  runStarting.value = true;
  try {
    runId.value = await pipelineApi.runPipeline(
      pipeline.value,
      repos,
      onFailure.value,
    );
    report.value = null;
    expandedSteps.value = new Set();
    await refreshReport();
    ElMessage.success("Pipeline 已提交运行");
  } catch (e) {
    ElMessage.error(`运行失败: ${e}`);
  } finally {
    runStarting.value = false;
  }
}

async function cancelRun() {
  if (!runId.value) return;
  try {
    await pipelineApi.cancelDag(runId.value);
    ElMessage.info("已请求取消（运行中的节点将协作式停止）");
    await refreshReport();
  } catch (e) {
    ElMessage.error(`取消失败: ${e}`);
  }
}

async function refreshReport() {
  if (!runId.value) return;
  try {
    report.value = await pipelineApi.getPipelineRun(
      runId.value,
      pipeline.value,
    );
    // Auto-expand failed/partial steps for triage.
    const next = new Set(expandedSteps.value);
    for (const s of report.value.steps) {
      if (s.failed > 0 || s.status === "partialSuccess") next.add(s.stepId);
    }
    expandedSteps.value = next;
  } catch (e) {
    console.error("Failed to load run report:", e);
  }
}

function toggleStepDetail(stepId: string) {
  const next = new Set(expandedSteps.value);
  if (next.has(stepId)) next.delete(stepId);
  else next.add(stepId);
  expandedSteps.value = next;
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

function kindLabel(step: PipelineStep): string {
  switch (step.kind.type) {
    case "fetch":
      return "Fetch";
    case "checkStatus":
      return "Check Status";
    case "pull":
      return "Pull";
    case "build":
      return `Build: ${step.kind.command || "?"}`;
    case "test":
      return `Test: ${step.kind.command || "?"}`;
    case "report":
      return "Report（汇聚）";
  }
}

function statusLabel(s: string): string {
  const map: Record<string, string> = {
    queued: "排队中",
    pending: "等待中",
    running: "执行中",
    success: "成功",
    partialSuccess: "部分成功",
    failed: "失败",
    cancelled: "已取消",
    skipped: "已跳过",
  };
  return map[s] ?? s;
}

function statusTagType(
  s: string,
): "success" | "warning" | "danger" | "info" | "primary" {
  switch (s) {
    case "success":
      return "success";
    case "partialSuccess":
    case "running":
      return "warning";
    case "failed":
      return "danger";
    default:
      return "info";
  }
}

function itemMark(s: string): string {
  switch (s) {
    case "success":
      return "✓";
    case "failed":
      return "✗";
    case "cancelled":
      return "⊘";
    case "skipped":
      return "⤼";
    default:
      return "…";
  }
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 100) / 10;
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  return `${m}m${Math.round(s % 60)}s`;
}

function goBack() {
  router.push({ name: "dashboard" });
}
</script>

<style scoped>
.pipeline-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  box-sizing: border-box;
  overflow: hidden;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  padding-bottom: 10px;
  border-bottom: 1px solid #ebeef5;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.main-area {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: 12px;
  padding-top: 10px;
}

.editor-panel {
  width: 340px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid #ebeef5;
  border-radius: 6px;
  overflow: hidden;
}

.panel-title {
  font-size: 13px;
  font-weight: 600;
  color: #303133;
  padding: 8px 10px;
  border-bottom: 1px solid #ebeef5;
  background: #fafafa;
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.editor-scroll {
  flex: 1;
  min-height: 0;
  padding: 8px;
}

.step-card {
  border: 1px solid #ebeef5;
  border-radius: 6px;
  margin-bottom: 8px;
  padding: 6px 8px;
}

.step-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.step-order {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: #409eff;
  color: #fff;
  font-size: 11px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
}

.step-name {
  flex: 1;
}

.step-body {
  margin-top: 6px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.step-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.step-label {
  font-size: 12px;
  color: #909399;
  width: 48px;
  flex-shrink: 0;
}

.add-step {
  width: 100%;
}

.stage-panel {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.run-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.graph-box {
  border: 1px solid #ebeef5;
  border-radius: 6px;
  overflow: hidden;
  flex-shrink: 0;
}

.run-summary {
  font-size: 12px;
  font-weight: 400;
  color: #606266;
  display: inline-flex;
  align-items: center;
  gap: 4px;
}

.graph-canvas {
  position: relative;
  display: flex;
  gap: 56px;
  padding: 16px 20px;
  overflow-x: auto;
  min-height: 120px;
  background: #fdfdfd;
}

.graph-edges {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.edge-path {
  stroke: #c0c4cc;
  stroke-width: 1.5;
  fill: none;
}

.graph-layer {
  display: flex;
  flex-direction: column;
  gap: 12px;
  justify-content: center;
  z-index: 1;
}

.gnode {
  width: 150px;
  border: 1px solid #dcdfe6;
  border-radius: 6px;
  background: #fff;
  padding: 6px 8px;
  font-size: 12px;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.04);
}

.gnode-name {
  font-weight: 600;
  color: #303133;
}

.gnode-kind {
  color: #909399;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.gnode-cond {
  color: #e6a23c;
  font-size: 11px;
}

.gnode.is-report {
  border-style: dashed;
}

.node-running {
  border-color: #e6a23c;
  background: #fdf6ec;
}

.node-success {
  border-color: #67c23a;
  background: #f0f9eb;
}

.node-partialSuccess {
  border-color: #e6a23c;
  background: #fdf6ec;
}

.node-failed {
  border-color: #f56c6c;
  background: #fef0f0;
}

.node-cancelled,
.node-skipped {
  border-color: #c0c4cc;
  background: #f4f4f5;
}

.graph-empty {
  color: #c0c4cc;
  font-size: 13px;
  align-self: center;
  margin: auto;
}

.report-box {
  flex: 1;
  min-height: 0;
  border: 1px solid #ebeef5;
  border-radius: 6px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.report-scroll {
  flex: 1;
  min-height: 0;
  padding: 8px;
}

.report-step {
  border: 1px solid #ebeef5;
  border-radius: 6px;
  margin-bottom: 8px;
}

.report-step-head {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  cursor: pointer;
  flex-wrap: wrap;
}

.report-step-name {
  font-weight: 600;
  font-size: 13px;
}

.report-step-kind {
  color: #909399;
  font-size: 11px;
}

.report-step-stats {
  color: #606266;
  font-size: 12px;
}

.report-step-dur {
  color: #909399;
  font-size: 12px;
}

.report-step-toggle {
  margin-left: auto;
  color: #409eff;
  font-size: 12px;
}

.report-items {
  border-top: 1px dashed #ebeef5;
  padding: 4px 10px 6px;
}

.report-item {
  display: flex;
  align-items: baseline;
  gap: 8px;
  font-size: 12px;
  padding: 2px 0;
  flex-wrap: wrap;
}

.item-mark {
  width: 14px;
  text-align: center;
  flex-shrink: 0;
}

.mark-success {
  color: #67c23a;
}

.mark-failed {
  color: #f56c6c;
}

.mark-cancelled,
.mark-skipped {
  color: #909399;
}

.item-repo {
  font-weight: 500;
}

.item-status {
  color: #606266;
}

.item-attempts,
.item-dur {
  color: #909399;
}

.item-msg {
  color: #f56c6c;
  word-break: break-all;
}

.item-output {
  width: 100%;
  font-family: Consolas, monospace;
  font-size: 11px;
  color: #606266;
  background: #f5f7fa;
  padding: 4px 8px;
  border-radius: 3px;
  margin: 2px 0 0;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 120px;
  overflow-y: auto;
}
</style>
