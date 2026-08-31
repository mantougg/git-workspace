<template>
  <div class="runtime-dashboard">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button
          type="primary"
          :disabled="!workspaceStore.currentWorkspace"
          @click="router.push({ name: 'runtime-app-wizard' })"
        >
          <template #icon><n-icon><AddOutline /></n-icon></template>
          新建应用
        </n-button>
        <n-button :loading="store.loading" @click="reload">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
        <n-button
          :disabled="!workspaceStore.currentWorkspace"
          :loading="resolving"
          @click="onResolve"
        >
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          解析依赖
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-button
          type="info"
          :disabled="!selectedConfig || !workspaceStore.currentWorkspace"
          @click="openCurrentDiagnostic"
        >
          <template #icon><n-icon><SparklesOutline /></n-icon></template>
          AI 诊断
        </n-button>
        <n-button
          :disabled="!selectedConfig || !workspaceStore.currentWorkspace"
          @click="openRuntimeAssistant"
        >
          <template #icon><n-icon><SparklesOutline /></n-icon></template>
          AI 助手
        </n-button>
        <n-button
          :disabled="!selectedConfig"
          @click="portModalShow = true"
        >
          <template #icon><n-icon><GitNetworkOutline /></n-icon></template>
          端口诊断
        </n-button>
        <n-button
          type="success"
          :disabled="!workspaceStore.currentWorkspace || store.configs.length === 0"
          @click="onStartAll"
        >
          <template #icon><n-icon><PlayOutline /></n-icon></template>
          全部启动
        </n-button>
        <n-button
          type="error"
          :disabled="!workspaceStore.currentWorkspace || store.processes.length === 0"
          @click="onStopAll"
        >
          <template #icon><n-icon><StopOutline /></n-icon></template>
          全部停止
        </n-button>
      </div>
    </div>

    <!-- R-14 §80：可行动错误横幅（Reason + 上下文 + Suggested Actions） -->
    <RuntimeErrorAlert
      v-if="lastError"
      :error="lastError"
      @dismiss="clearError"
      @confirm-script="onConfirmScript"
      @open-logs="onAlertOpenLogs"
      @retry="pendingRetry?.()"
      @ai-analyze="onAlertAiAnalyze"
    />

    <RuntimeDiagnosticAssistant
      :request="diagnosticRequest"
      @configure="router.push({ name: 'ai-settings' })"
    />

    <!-- R-21 §47/§48：Runtime Dependency Changed 提示条（snooze 支持） -->
    <n-alert
      v-for="[name, payload] in store.dependencyChanged"
      :key="name"
      type="warning"
      class="dep-changed-alert"
      closable
      @close="store.dismissDependencyChanged(name)"
    >
      <template #header>
        Runtime Dependency Changed · {{ name }}
        <n-tag size="small" :bordered="false" :type="payload.reason === 'branchSwitched' ? 'info' : 'warning'">
          {{ payload.reason === "branchSwitched" ? "分支切换 · POM 变化" : "仓库文件修改" }}
        </n-tag>
      </template>
      <n-space vertical :size="4">
        <span class="mono dep-changed-line">
          受影响模块：{{ payload.affectedModules.join("、") || "—" }}
        </span>
        <span class="mono dep-changed-line">仓库：{{ payload.repos.join("、") }}</span>
        <n-space :size="8">
          <n-button size="small" type="primary" @click="onRebuildRestart(name)">
            Rebuild &amp; Restart
          </n-button>
          <n-button size="small" @click="store.dismissDependencyChanged(name)">稍后</n-button>
        </n-space>
      </n-space>
    </n-alert>

    <!-- D-11 摘要行：高密度平铺（与 DashboardView summary-strip 同模式） -->
    <n-spin :show="store.loading">
      <div class="summary-strip">
        <span class="summary-item">
          <span class="summary-value">{{ store.configs.length }}</span>
          <span class="summary-label">应用配置 · Runtime 配置总数</span>
        </span>
        <span class="summary-item tone-ok">
          <span class="summary-value">{{ runningCount }}</span>
          <span class="summary-label">运行中 · Running</span>
        </span>
        <span class="summary-item tone-warn">
          <span class="summary-value">{{ startingCount }}</span>
          <span class="summary-label">启动中 · Preparing / Building / Starting</span>
        </span>
        <span class="summary-item tone-danger">
          <span class="summary-value">{{ failedCount }}</span>
          <span class="summary-label">失败 · Failed</span>
        </span>
        <span class="summary-item tone-info">
          <span class="summary-value">{{ store.processes.length }}</span>
          <span class="summary-label">进程记录 · Maven 项目索引 {{ store.projects.length }} 个</span>
        </span>
        <span
          v-if="watchSummary"
          class="summary-item"
          :class="watchSummary.ok ? 'tone-ok' : 'tone-danger'"
        >
          <span class="summary-value">R-17</span>
          <span class="summary-label">{{ watchSummary.label }}</span>
        </span>
      </div>
    </n-spin>

    <!-- Applications -->
    <Panel>
      <template #header>
        <span>Applications</span>
        <span v-if="store.configs.length === 0" class="section-hint">
          暂无 Runtime 配置 —— 点击「新建应用」创建（向导会自动预填 JDK / Main Class / Profile）
        </span>
        <span v-else-if="store.projects.length === 0" class="section-hint warn-hint">
          依赖索引为空，请先「解析依赖」后再启动
        </span>
      </template>
      <n-spin :show="store.loading">
        <n-data-table
          :columns="configColumns"
          :data="store.configs"
          :row-key="(row: RuntimeConfigSummary) => row.id"
          :row-class-name="() => ''"
          :single-line="false"
          size="small"
          @row-click="onSelectRow"
        />
      </n-spin>
    </Panel>

    <!-- Selected application detail -->
    <Panel v-if="selectedConfig">
      <template #header>
        <span>应用详情 · {{ selectedConfig.name }}</span>
        <n-tag v-if="configDetail" size="small" class="scope-tag" type="info">
          Scope: {{ scopeLabel(configDetail.scope) }}
        </n-tag>
      </template>
      <n-descriptions :column="4" label-placement="left" bordered size="small">
        <n-descriptions-item label="JDK">
          {{ configDetail?.jdk ?? selectedConfig.jdk ?? "—" }}
        </n-descriptions-item>
        <n-descriptions-item label="Profile">
          {{ configDetail?.profile ?? selectedConfig.profile ?? "—" }}
        </n-descriptions-item>
        <n-descriptions-item label="PID">
          {{ processOf(selectedConfig.name)?.pid ?? "—" }}
        </n-descriptions-item>
        <n-descriptions-item label="端口">
          {{ processOf(selectedConfig.name)?.ports?.join(", ") || "—" }}
        </n-descriptions-item>
        <n-descriptions-item label="内存" :span="1">
          {{
            processOf(selectedConfig.name)?.memoryBytes
              ? formatBytes(processOf(selectedConfig.name)!.memoryBytes!)
              : "—"
          }}
        </n-descriptions-item>
        <n-descriptions-item label="CPU" :span="1">
          {{
            processOf(selectedConfig.name)?.cpuPercent != null
              ? processOf(selectedConfig.name)!.cpuPercent!.toFixed(1) + "%"
              : "—"
          }}
        </n-descriptions-item>
        <n-descriptions-item label="运行策略" :span="2">
          {{ processOf(selectedConfig.name)?.runStrategy ?? "—" }}
        </n-descriptions-item>
        <n-descriptions-item label="VM Options" :span="4">
          <span v-if="configDetail?.vmOptions?.length" class="mono">{{
            configDetail.vmOptions.join(" ")
          }}</span>
          <span v-else class="muted">—</span>
        </n-descriptions-item>
        <n-descriptions-item label="Program Args" :span="4">
          <span v-if="configDetail?.programArguments?.length" class="mono">{{
            configDetail.programArguments.join(" ")
          }}</span>
          <span v-else class="muted">—</span>
        </n-descriptions-item>
        <n-descriptions-item label="启动命令预览" :span="4">
          <span v-if="processOf(selectedConfig.name)?.commandPreview" class="mono cmd-preview">
            {{ processOf(selectedConfig.name)!.commandPreview }}
          </span>
          <span v-else class="muted">—</span>
        </n-descriptions-item>
        <n-descriptions-item label="环境变量" :span="4">
          <span v-if="configDetail && envKeys(configDetail).length > 0" class="mono env-summary">
            {{ envKeys(configDetail).join(", ") }}
          </span>
          <span v-else class="muted">—</span>
        </n-descriptions-item>
      </n-descriptions>
    </Panel>

    <!-- Processes -->
    <Panel title="Processes">
      <n-spin :show="store.loading">
        <n-data-table
          :columns="processColumns"
          :data="store.processes"
          size="small"
          :single-line="false"
        />
      </n-spin>
    </Panel>

    <!-- R-14 §75：脚本执行确认管理（默认禁止自动执行；可重置） -->
    <Panel>
      <template #header>
        <span>脚本执行确认（§75 Command Safety）</span>
      </template>
      <template #actions>
        <n-button size="small" :loading="approvalsLoading" @click="loadApprovals">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
        <n-popconfirm @positive-click="onResetApprovals(store.workspaceId, null)">
          <template #trigger>
            <n-button
              size="small"
              type="error"
              :disabled="workspaceApprovals.length === 0"
            >
              全部重置
            </n-button>
          </template>
          撤销当前 workspace 的全部脚本确认？
        </n-popconfirm>
      </template>
      <n-spin :show="approvalsLoading">
        <n-data-table
          :columns="approvalColumns"
          :data="workspaceApprovals"
          size="small"
          :single-line="false"
        />
      </n-spin>
      <div class="section-hint">
        脚本**默认禁止自动执行**：首次执行必须确认；脚本内容变更后需重新确认；「不再询问」可随时重置（全局约束 §3）。
      </div>
    </Panel>

    <!-- Scheduler config (§66) -->
    <Panel title="Runtime Task Scheduler（§66 限流并发）">
      <div class="scheduler-row">
        <div class="scheduler-field">
          <span class="scheduler-label">最大并发 Build</span>
          <n-input-number
            v-model:value="scheduler.maxConcurrentBuilds"
            :min="1"
            :max="16"
            size="small"
          />
        </div>
        <div class="scheduler-field">
          <span class="scheduler-label">最大并发 Resolve</span>
          <n-input-number
            v-model:value="scheduler.maxConcurrentResolves"
            :min="1"
            :max="16"
            size="small"
          />
        </div>
        <n-button size="small" type="primary" :loading="savingScheduler" @click="onSaveScheduler">
          保存并生效
        </n-button>
      </div>
    </Panel>

    <!-- R-16 §81 端口诊断：占用检测 / Kill（确认）/ 改写端口 -->
    <PortDiagnosticsModal
      v-model:show="portModalShow"
      :runtime-name="portModalTarget"
      :default-port="portModalDefaultPort"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, NTag, NIcon, NTooltip, useMessage, useDialog } from "naive-ui";
import {
  AddOutline,
  RefreshOutline,
  PlayOutline,
  StopOutline,
  GitNetworkOutline,
  SparklesOutline,
} from "@vicons/ionicons5";
import Panel from "@/components/shell/Panel.vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRuntimeStore } from "@/stores/runtime";
import * as runtimeApi from "@/api/runtime";
import { nodeListProjects } from "@/api/node";
import type {
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  RuntimeProcessInfo,
  SchedulerConfig,
} from "@/types/runtime";
import type { RuntimeScope } from "@/types/maven";
import type { NodeProjectNode } from "@/types/node";
import { errMsg } from "@/utils/error";
import RuntimeErrorAlert from "@/components/runtime/RuntimeErrorAlert.vue";
import PortDiagnosticsModal from "@/components/runtime/PortDiagnosticsModal.vue";
import RuntimeDiagnosticAssistant from "@/components/ai/RuntimeDiagnosticAssistant.vue";
import { useAiAssistant } from "@/composables/useAiAssistant";
import type { ScriptApproval } from "@/types/runtime";
import type { DiagnosticErrorInput, RuntimeDiagnosticRequest } from "@/types/ai";

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const store = useRuntimeStore();
const message = useMessage();
const dialog = useDialog();
const { openAssistant } = useAiAssistant();

const resolving = ref(false);
const selectedConfig = ref<RuntimeConfigSummary | null>(null);
/** 选中应用的完整配置（按需加载）。 */
const configDetail = ref<RuntimeApplicationConfig | null>(null);
const scheduler = ref<SchedulerConfig>({ maxConcurrentBuilds: 2, maxConcurrentResolves: 4 });
const savingScheduler = ref(false);
const nodeProjects = ref<NodeProjectNode[]>([]);

// R-16 §81 端口诊断对话框（选中应用 + 默认端口取其探测端口）。
const portModalShow = ref(false);
const portModalTarget = computed(() => selectedConfig.value?.name ?? "");
const portModalDefaultPort = computed(() => {
  const ports = processOf(selectedConfig.value?.name ?? "")?.ports;
  return ports?.length ? ports[ports.length - 1] : 8080;
});

// ------------------------------------------------------------------
// R-14 §80 可行动错误提示 + §75 脚本确认流
// ------------------------------------------------------------------

/** 最近一次操作的结构化错误（页面顶部横幅）。 */
const lastError = ref<unknown>(null);
/** 失败的操作名。 */
const failedAction = ref("");
/** 确认脚本后自动重试的操作。 */
const pendingRetry = ref<(() => Promise<void>) | null>(null);
const diagnosticRequest = ref<RuntimeDiagnosticRequest | null>(null);

/** 统一错误处理：写入可行动错误横幅 + 轻提示。 */
function handleError(action: string, e: unknown, retry?: () => Promise<void>) {
  lastError.value = e;
  failedAction.value = action;
  pendingRetry.value = retry ?? null;
  message.error(`${action}失败：${errMsg(e)}`);
}

function clearError() {
  lastError.value = null;
  failedAction.value = "";
  pendingRetry.value = null;
}

function diagnosticProcessId(runtimeName: string, details?: Record<string, unknown> | null): number | null {
  const pid = typeof details?.pid === "number" ? details.pid : Number(details?.pid);
  const process = store.processes.find(
    (item) => item.runtimeName === runtimeName && (Number.isNaN(pid) || item.pid === pid),
  );
  return process?.processId ?? null;
}

function openDiagnostic(request: RuntimeDiagnosticRequest) {
  diagnosticRequest.value = request;
}

function openCurrentDiagnostic() {
  const config = selectedConfig.value;
  if (!config || store.workspaceId == null) return;
  const process = processOf(config.name);
  openDiagnostic({
    workspaceId: store.workspaceId,
    runtimeName: config.name,
    processId: process?.processId ?? null,
    project: config.project,
    wantConfigAdvice: true,
  });
}

function openRuntimeAssistant() {
  const config = selectedConfig.value;
  if (!config || store.workspaceId == null) return;
  const process = processOf(config.name);
  openAssistant({
    workspaceId: store.workspaceId,
    runtimeName: config.name,
    processId: process?.processId ?? null,
    inferredRole: "runtimeDiagnostician",
    origin: `Runtime Dashboard · ${config.name}`,
    draft: "请解释当前 Runtime 的状态、风险和建议的排查步骤。",
  });
}

function onAlertAiAnalyze(input: DiagnosticErrorInput) {
  if (store.workspaceId == null) return;
  const runtimeName = typeof input.details?.runtime === "string"
    ? input.details.runtime
    : selectedConfig.value?.name ?? "";
  if (!runtimeName) {
    message.warning("请先选择 Runtime 应用，再开始 AI 分析");
    return;
  }
  openAssistant({
    workspaceId: store.workspaceId,
    runtimeName,
    processId: diagnosticProcessId(runtimeName, input.details),
    inferredRole: "runtimeDiagnostician",
    origin: "Runtime Error Alert",
    supplementary: [{
      role: "userNote",
      kind: "error",
      sourceId: `runtime:error:${input.occurredAt ?? "latest"}`,
      displayName: `Runtime 错误：${input.code}`,
      content: JSON.stringify(input),
    }],
    draft: "请分析这个 Runtime 错误，区分确定性事实与排查建议。",
  });
}

/** §75：用户在横幅确认脚本后，批准并自动重试原操作。 */
async function onConfirmScript(details: Record<string, unknown>) {
  const workspaceId = Number(details.workspaceId ?? store.workspaceId);
  const runtimeName = String(details.runtimeName ?? "");
  const scriptType = String(details.scriptType ?? "");
  const preview = String(details.preview ?? "");
  if (!workspaceId || !runtimeName || !scriptType) {
    message.error("缺少脚本确认所需信息");
    return;
  }
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "确认执行脚本",
        content: `Runtime「${runtimeName}」的 ${scriptType === "pre" ? "Pre-Build" : "Post-Build"} 脚本需要确认：\n\n${preview}\n\n确认后立即执行并记录；可随时在下方「脚本执行确认」中重置。`,
        positiveText: "确认并执行",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject(new Error("cancelled")),
        onClose: () => reject(new Error("cancelled")),
      });
    });
  } catch {
    return; // 用户取消
  }
  try {
    await runtimeApi.runtimeApproveScript(workspaceId, runtimeName, scriptType);
    message.success("已确认脚本，正在重试操作…");
    await loadApprovals();
    const retry = pendingRetry.value;
    clearError();
    if (retry) {
      await retry();
    }
  } catch (e) {
    message.error("确认脚本失败：" + errMsg(e));
  }
}

/** 脚本确认管理（「不再询问」可重置，§75）。 */
const approvals = ref<ScriptApproval[]>([]);
const approvalsLoading = ref(false);

async function loadApprovals() {
  approvalsLoading.value = true;
  try {
    approvals.value = await runtimeApi.runtimeGetScriptApprovals();
  } catch (e) {
    console.error("R-14: load script approvals failed:", e);
  } finally {
    approvalsLoading.value = false;
  }
}

async function onResetApprovals(workspaceId: number | null, runtimeName: string | null) {
  try {
    const removed = await runtimeApi.runtimeResetScriptApprovals(workspaceId, runtimeName);
    message.success(`已撤销 ${removed} 条脚本确认`);
    await loadApprovals();
  } catch (e) {
    message.error("重置失败：" + errMsg(e));
  }
}

const workspaceApprovals = computed(() =>
  store.workspaceId != null
    ? approvals.value.filter((a) => a.workspaceId === store.workspaceId)
    : [],
);

// ------------------------------------------------------------------
// 状态可视化（§90）
// ------------------------------------------------------------------

function processOf(name: string): RuntimeProcessInfo | undefined {
  return store.processes.find((p) => p.runtimeName === name);
}

function isBusy(name: string): boolean {
  const p = processOf(name);
  if (!p) return false;
  return ["preparing", "resolving", "building", "starting", "stopping"].includes(p.status);
}

function isRunning(name: string): boolean {
  const p = processOf(name);
  return !!p && (p.status === "running" || p.status === "starting");
}

interface StatusView {
  label: string;
  cls: string;
}

function statusOf(name: string): StatusView {
  const p = processOf(name);
  if (!p) {
    return { label: "Stopped", cls: "stopped" };
  }
  switch (p.status) {
    case "preparing":
    case "resolving":
    case "starting":
      return { label: "Preparing", cls: "preparing" };
    case "building":
      return { label: "Building", cls: "building" };
    case "running": {
      // R-16：探针状态机取值优先展示；无探针时回落 up/down 生命周期推导。
      const health = store.health.get(name);
      if (health === "unhealthy") return { label: "Unhealthy", cls: "unhealthy" };
      if (health === "starting") return { label: "Starting (Health)", cls: "preparing" };
      if (health === "healthy" || health === "up" || health == null) {
        return { label: "Running", cls: "running" };
      }
      return { label: "Unhealthy", cls: "unhealthy" };
    }
    case "stopping":
      return { label: "Stopping", cls: "stopping" };
    case "failed":
      return { label: "Failed", cls: "failed" };
    default:
      return { label: p.status, cls: "other" };
  }
}

/** §65 Start 流程阶段文案：Preparing... / Building... / Starting... */
function stageOf(name: string): string | null {
  const stage = store.stages.get(name);
  if (!stage) return null;
  const map: Record<string, string> = {
    preparing: "Preparing...",
    resolving: "Resolving dependencies...",
    building: "Building...",
    starting: "Starting...",
  };
  return map[stage] ?? stage;
}

const runningCount = computed(
  () => store.configs.filter((c) => isRunning(c.name)).length,
);

/** R-21 §47/§48：从联动提示一键 Rebuild & Restart（复用 RebuildRestart 任务）。 */
async function onRebuildRestart(name: string) {
  try {
    await store.rebuildRestart(name);
    store.dismissDependencyChanged(name);
  } catch (e) {
    message.error("Rebuild & Restart 提交失败：" + errMsg(e));
  }
}
const startingCount = computed(
  () => store.configs.filter((c) => {
    const p = processOf(c.name);
    return !!p && ["preparing", "resolving", "building", "starting"].includes(p.status);
  }).length,
);
const failedCount = computed(
  () => store.configs.filter((c) => processOf(c.name)?.status === "failed").length,
);

// R-17：File Watch / 自动重启活动摘要（事件驱动；无活动时不显示该槽位）。
const watchSummary = computed(() => {
  const restart = store.lastRestart;
  const change = store.lastFileChange;
  if (!restart && !change) return null;
  if (restart) {
    const label = restart.success
      ? `自动重启成功 · ${restart.runtimeName}`
      : `自动重启失败 · ${restart.runtimeName}`;
    return { label, ok: restart.success };
  }
  return {
    label: `File Watch 变更 ${change!.paths.length} 个文件 · ${change!.at.slice(11, 19)}`,
    ok: true,
  };
});

function scopeLabel(scope: RuntimeScope): string {
  switch (scope.mode) {
    case "auto":
      return "Auto";
    case "manual":
      return `Manual (${scope.projectIds.length})`;
    case "hybrid":
      return `Hybrid (+${scope.includeProjectIds.length} / -${scope.excludeProjectIds.length})`;
  }
}

function envKeys(config: RuntimeApplicationConfig): string[] {
  return Object.keys(config.environment);
}

function nodeLaunchLabel(row: RuntimeConfigSummary): string {
  const detail = store.configDetails.get(row.name);
  const project = nodeProjects.value.find(
    (candidate) => normalizePath(candidate.path) === normalizePath(row.project),
  );
  const manager = detail?.nodePackageManager || project?.packageManager || "npm";
  const script = detail?.nodeScript || "script";
  return `${manager} run ${script}`;
}

function normalizePath(path: string): string {
  return path.replace(/\\/g, "/");
}

// ------------------------------------------------------------------
// n-data-table columns definitions
// ------------------------------------------------------------------

const configColumns = [
  {
    title: "状态",
    key: "status",
    width: 130,
    render(row: RuntimeConfigSummary) {
      const status = statusOf(row.name);
      const stage = stageOf(row.name);
      return h("div", [
        h("span", { class: "status-cell" }, [
          h("span", { class: `status-dot ${status.cls}` }),
          status.label,
        ]),
        stage ? h("div", { class: "stage-text" }, stage) : null,
      ]);
    },
  },
  {
    title: "名称",
    key: "name",
    minWidth: 120,
    render(row: RuntimeConfigSummary) {
      return h("span", { class: "app-name" }, row.name);
    },
  },
  {
    title: "项目",
    key: "project",
    minWidth: 180,
    ellipsis: { tooltip: true },
    render(row: RuntimeConfigSummary) {
      return h("span", { class: "mono" }, row.project);
    },
  },
  // F-23：启动方式 = 闭包是否含工作区源码依赖（与 Build 流水线同一数据源）。
  {
    title: "启动方式",
    key: "launchMode",
    width: 120,
    render(row: RuntimeConfigSummary) {
      if (row.kind === "node") {
        return h(
          NTag,
          { size: "small", type: "info" },
          { default: () => nodeLaunchLabel(row) },
        );
      }
      const info = store.closureInfo.get(row.name);
      if (!info) {
        return h(
          NTooltip,
          { trigger: "hover" },
          {
            trigger: () => h("span", { class: "muted" }, "—"),
            default: () => "未解析依赖，点上方「解析依赖」后可见",
          },
        );
      }
      if (info.sourceCount === 0) {
        return h(NTag, { size: "small" }, { default: () => "直接启动" });
      }
      return h(
        NTooltip,
        { trigger: "hover" },
        {
          trigger: () =>
            h(
              NTag,
              { size: "small", type: "success" },
              { default: () => `源码启动 ×${info.sourceCount}` },
            ),
          default: () => `源码依赖：${info.sourceNames.join("、")}`,
        },
      );
    },
  },
  {
    title: "Main Class",
    key: "mainClass",
    minWidth: 200,
    ellipsis: { tooltip: true },
    render(row: RuntimeConfigSummary) {
      return row.mainClass
        ? h("span", { class: "mono" }, row.mainClass)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "JDK",
    key: "jdk",
    width: 70,
    render(row: RuntimeConfigSummary) {
      return row.jdk
        ? h("span", { class: "mono" }, row.jdk)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "Profile",
    key: "profile",
    width: 90,
    render(row: RuntimeConfigSummary) {
      return row.profile
        ? h(NTag, { size: "small", type: "warning" }, { default: () => row.profile })
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "PID",
    key: "pid",
    width: 80,
    render(row: RuntimeConfigSummary) {
      const p = processOf(row.name);
      return p?.pid
        ? h("span", { class: "mono" }, String(p.pid))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "端口",
    key: "ports",
    width: 90,
    render(row: RuntimeConfigSummary) {
      const p = processOf(row.name);
      return p?.ports?.length
        ? h("span", { class: "mono" }, p.ports.join(","))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 330,
    fixed: "right" as const,
    render(row: RuntimeConfigSummary) {
      return h("div", { style: "display:flex;gap:4px;flex-wrap:wrap" }, [
        h(
          NButton,
          {
            size: "small",
            type: "primary",
            disabled: isBusy(row.name),
            onClick: () => onStart(row.name),
          },
          { default: () => "启动" },
        ),
        h(
          NButton,
          {
            size: "small",
            disabled: !isRunning(row.name),
            onClick: () => onStop(row.name),
          },
          { default: () => "停止" },
        ),
        h(
          NButton,
          {
            size: "small",
            disabled: !isRunning(row.name),
            onClick: () => onRestart(row.name),
          },
          { default: () => "重启" },
        ),
        h(
          NButton,
          {
            size: "small",
            disabled: isBusy(row.name),
            onClick: () => onBuild(row.name),
          },
          { default: () => "构建" },
        ),
        h(
          NButton,
          {
            size: "small",
            text: true,
            type: "primary",
            onClick: () => openLogs(row.name),
          },
          { default: () => "日志" },
        ),
        h(
          NButton,
          {
            size: "small",
            text: true,
            onClick: () => openWizard(row.name),
          },
          { default: () => "配置" },
        ),
        h(
          NButton,
          {
            size: "small",
            text: true,
            type: "error",
            onClick: () => {
              dialog.error({
                title: "确认删除",
                content: "确定删除该 Runtime 配置吗？",
                positiveText: "删除",
                negativeText: "取消",
                onPositiveClick: () => onDelete(row),
              });
            },
          },
          { default: () => "删除" },
        ),
      ]);
    },
  },
];

const processColumns = [
  { title: "ID", key: "processId", width: 70 },
  { title: "Runtime", key: "runtimeName", minWidth: 120 },
  {
    title: "PID",
    key: "pid",
    width: 90,
    render(row: RuntimeProcessInfo) {
      return h("span", { class: "mono" }, row.pid != null ? String(row.pid) : "—");
    },
  },
  {
    title: "状态",
    key: "status",
    width: 110,
    render(row: RuntimeProcessInfo) {
      const status = statusOf(row.runtimeName);
      return h("span", { class: "status-cell" }, [
        h("span", { class: `status-dot ${status.cls}` }),
        status.label,
      ]);
    },
  },
  {
    title: "策略",
    key: "runStrategy",
    width: 110,
    render(row: RuntimeProcessInfo) {
      return row.runStrategy
        ? h(NTag, { size: "small" }, { default: () => row.runStrategy })
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "端口",
    key: "ports",
    width: 110,
    render(row: RuntimeProcessInfo) {
      return h("span", { class: "mono" }, row.ports?.join(", ") || "—");
    },
  },
  {
    title: "已运行",
    key: "uptime",
    width: 110,
    render(row: RuntimeProcessInfo) {
      return row.uptimeSeconds != null
        ? h("span", {}, formatUptime(row.uptimeSeconds))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "CPU",
    key: "cpu",
    width: 80,
    render(row: RuntimeProcessInfo) {
      return row.cpuPercent != null
        ? h("span", {}, `${row.cpuPercent.toFixed(1)}%`)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "内存",
    key: "memory",
    width: 100,
    render(row: RuntimeProcessInfo) {
      return row.memoryBytes != null
        ? h("span", {}, formatBytes(row.memoryBytes))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "退出码",
    key: "exitCode",
    width: 80,
    render(row: RuntimeProcessInfo) {
      return row.exitCode != null
        ? h("span", { class: "mono" }, String(row.exitCode))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "孤儿接管",
    key: "adopted",
    width: 90,
    render(row: RuntimeProcessInfo) {
      return row.adopted
        ? h(NTag, { size: "small", type: "warning" }, { default: () => "已接管" })
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "启动于",
    key: "startedAt",
    width: 160,
    render(row: RuntimeProcessInfo) {
      return h("span", { class: "muted" }, formatTime(row.startedAt));
    },
  },
];

const approvalColumns = [
  {
    title: "Runtime",
    key: "runtimeName",
    minWidth: 120,
    render(row: ScriptApproval) {
      return h("span", { class: "app-name" }, row.runtimeName);
    },
  },
  {
    title: "类型",
    key: "scriptType",
    width: 110,
    render(row: ScriptApproval) {
      return h(
        NTag,
        { size: "small", type: row.scriptType === "pre" ? "warning" : "success" },
        { default: () => (row.scriptType === "pre" ? "Pre-Build" : "Post-Build") },
      );
    },
  },
  {
    title: "脚本预览",
    key: "preview",
    minWidth: 220,
    ellipsis: { tooltip: true },
    render(row: ScriptApproval) {
      return h("span", { class: "mono" }, row.preview);
    },
  },
  {
    title: "确认于",
    key: "approvedAt",
    width: 160,
    render(row: ScriptApproval) {
      return h("span", { class: "muted" }, formatTime(row.approvedAt));
    },
  },
  {
    title: "最近执行",
    key: "lastExecutedAt",
    width: 160,
    render(row: ScriptApproval) {
      return row.lastExecutedAt
        ? h("span", { class: "muted" }, formatTime(row.lastExecutedAt))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 90,
    render(row: ScriptApproval) {
      return h(
        NButton,
        {
          size: "small",
          text: true,
          type: "error",
          onClick: () => onResetApprovals(row.workspaceId, row.runtimeName),
        },
        { default: () => "重置" },
      );
    },
  },
];

// ------------------------------------------------------------------
// 操作
// ------------------------------------------------------------------

async function onStart(name: string) {
  clearError();
  try {
    await store.start(name);
    message.success(`已提交启动任务：${name}`);
  } catch (e) {
    handleError("启动", e, () => onStart(name));
  }
}

async function onStop(name: string) {
  clearError();
  try {
    await store.stop(name);
    message.success(`已提交停止任务：${name}`);
  } catch (e) {
    handleError("停止", e);
  }
}

async function onRestart(name: string) {
  clearError();
  try {
    await store.restart(name);
    message.success(`已提交重启任务：${name}`);
  } catch (e) {
    handleError("重启", e, () => onRestart(name));
  }
}

async function onBuild(name: string) {
  clearError();
  try {
    await store.build(name);
    message.success(`已提交构建任务：${name}`);
  } catch (e) {
    handleError("构建", e, () => onBuild(name));
  }
}

async function onResolve() {
  if (!workspaceStore.currentWorkspace) return;
  clearError();
  resolving.value = true;
  try {
    const taskId = await store.resolveDependencies();
    message.success(`依赖解析任务已提交：${taskId}`);
  } catch (e) {
    handleError("依赖解析", e, () => onResolve());
  } finally {
    resolving.value = false;
  }
}

async function onStartAll() {
  clearError();
  try {
    const ids = await store.startEnvironment();
    message.success(`已提交 ${ids.length} 个启动任务`);
  } catch (e) {
    handleError("全部启动", e, () => onStartAll());
  }
}

async function onStopAll() {
  clearError();
  try {
    const ids = await store.stopEnvironment();
    message.success(`已提交 ${ids.length} 个停止任务`);
  } catch (e) {
    handleError("全部停止", e);
  }
}

async function onDelete(row: RuntimeConfigSummary) {
  try {
    await store.removeConfig(row.name);
    if (selectedConfig.value?.name === row.name) {
      selectedConfig.value = null;
      configDetail.value = null;
    }
    message.success(`已删除配置：${row.name}`);
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
  }
}

function onSelectRow(row: RuntimeConfigSummary) {
  if (!row) return;
  selectedConfig.value = row;
  configDetail.value = null;
  // 按需加载详情（IPC 打开 JSON 文件，勿全量拉取）。
  store
    .loadConfigDetail(row.name)
    .then((c) => {
      configDetail.value = c;
    })
    .catch((e) => console.error("R-13: load config detail failed:", e));
}

function openLogs(name: string) {
  router.push({ name: "runtime-logs", query: { name } });
}

function openWizard(name: string) {
  router.push({ name: "runtime-app-wizard", query: { edit: name } });
}

async function onSaveScheduler() {
  savingScheduler.value = true;
  try {
    await runtimeApi.runtimeSetSchedulerConfig(scheduler.value);
    message.success("调度并发上限已生效");
  } catch (e) {
    message.error("保存失败：" + errMsg(e));
  } finally {
    savingScheduler.value = false;
  }
}

// ------------------------------------------------------------------
// 数据加载
// ------------------------------------------------------------------

async function reload() {
  // F-15：store.workspaceId 派生自全局工作区并自动加载；显式刷新走 reloadAll。
  if (store.workspaceId == null) return;
  await store.reloadAll();
  if (store.configs.some((config) => config.kind === "node")) {
    try {
      nodeProjects.value = await nodeListProjects(store.workspaceId);
    } catch (e) {
      console.error("N-06: load Node project metadata failed:", e);
    }
  } else {
    nodeProjects.value = [];
  }
  try {
    scheduler.value = await runtimeApi.runtimeGetSchedulerConfig();
  } catch (e) {
    console.error("R-13: load scheduler config failed:", e);
  }
  await loadApprovals();
}

/** 错误横幅「查看日志」：优先打开失败应用（details.runtimeName），否则日志页。 */
function onAlertOpenLogs() {
  const raw = lastError.value as { details?: string } | null;
  let name = "";
  if (raw?.details) {
    try {
      name = String(JSON.parse(raw.details).runtimeName ?? "");
    } catch {
      // ignore
    }
  }
  if (name) {
    openLogs(name);
  } else {
    router.push({ name: "runtime-logs" });
  }
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  // F-15：先加载数据再订阅事件——订阅失败（如事件名非法）不得阻断数据展示。
  await reload();
  try {
    await store.subscribe();
  } catch (e) {
    console.error("R-13: runtime event subscribe failed:", e);
  }
});onUnmounted(() => {
  store.unsubscribe();
});
</script>

<style scoped>
.runtime-dashboard {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
  overflow-y: auto;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2);
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
  flex-wrap: wrap;
}
/* D-11 摘要行：数字+标签平铺，无卡片边框（同 DashboardView） */
.summary-strip {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-4);
  padding: var(--gw-space-2) var(--gw-space-4);
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
}
.summary-item {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.summary-value {
  font-size: 20px;
  font-weight: 600;
  line-height: 1.3;
}
.summary-label {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}
.tone-ok .summary-value {
  color: var(--gw-success);
}
.tone-warn .summary-value {
  color: var(--gw-warning);
}
.tone-danger .summary-value {
  color: var(--gw-danger);
}
.tone-info .summary-value {
  color: var(--gw-accent);
}
/* D-10：section 外壳已替换为 Panel 组件 */
.section-hint {
  font-size: 12px;
  color: var(--gw-text-dim);
}
.warn-hint {
  color: var(--gw-warning);
}
.status-cell {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}
.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  display: inline-block;
}
.status-dot.stopped {
  background: var(--gw-text-dim);
}
.status-dot.preparing,
.status-dot.starting,
.status-dot.stopping {
  background: var(--gw-accent);
}
.status-dot.building {
  background: var(--gw-warning);
}
.status-dot.running {
  background: var(--gw-success);
}
.status-dot.unhealthy {
  background: var(--gw-danger);
}
.status-dot.failed {
  background: var(--gw-danger);
}
.status-dot.other {
  background: var(--gw-text-dim);
}
.stage-text {
  font-size: 11px;
  color: var(--gw-accent);
  margin-top: 2px;
}
.app-name {
  font-weight: 600;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
}
.muted {
  color: var(--gw-text-dim);
}
.cmd-preview {
  word-break: break-all;
}
.env-summary {
  word-break: break-all;
}
.scope-tag {
  margin-left: 8px;
}
.scheduler-section {
  margin-bottom: 8px;
}
.scheduler-row {
  display: flex;
  gap: 20px;
  align-items: center;
  flex-wrap: wrap;
}
.scheduler-field {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}
.scheduler-label {
  font-size: 13px;
  color: var(--gw-text);
}
</style>
