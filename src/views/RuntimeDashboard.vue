<template>
  <div class="runtime-dashboard">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
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
    />

    <!-- Stat cards -->
    <n-spin :show="store.loading">
      <div class="cards">
        <div class="stat-card">
          <div class="stat-label">应用配置</div>
          <div class="stat-value">{{ store.configs.length }}</div>
          <div class="stat-sub">Runtime 配置总数</div>
        </div>
        <div class="stat-card tone-ok">
          <div class="stat-label">运行中</div>
          <div class="stat-value">{{ runningCount }}</div>
          <div class="stat-sub">● Running</div>
        </div>
        <div class="stat-card tone-warn">
          <div class="stat-label">启动中</div>
          <div class="stat-value">{{ startingCount }}</div>
          <div class="stat-sub">Preparing / Building / Starting</div>
        </div>
        <div class="stat-card tone-danger">
          <div class="stat-label">失败</div>
          <div class="stat-value">{{ failedCount }}</div>
          <div class="stat-sub">✕ Failed</div>
        </div>
        <div class="stat-card tone-info">
          <div class="stat-label">进程记录</div>
          <div class="stat-value">{{ store.processes.length }}</div>
          <div class="stat-sub">Maven 项目索引 {{ store.projects.length }} 个</div>
        </div>
      </div>
    </n-spin>

    <!-- Applications -->
    <div class="section">
      <div class="section-head">
        <div class="section-title">Applications</div>
        <div v-if="store.configs.length === 0" class="section-hint">
          暂无 Runtime 配置 —— 点击「新建应用」创建（向导会自动预填 JDK / Main Class / Profile）
        </div>
        <div v-else-if="store.projects.length === 0" class="section-hint warn-hint">
          依赖索引为空，请先「解析依赖」后再启动
        </div>
      </div>
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
    </div>

    <!-- Selected application detail -->
    <div v-if="selectedConfig" class="section">
      <div class="section-title">
        应用详情 · {{ selectedConfig.name }}
        <n-tag v-if="configDetail" size="small" class="scope-tag" type="info">
          Scope: {{ scopeLabel(configDetail.scope) }}
        </n-tag>
      </div>
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
    </div>

    <!-- Processes -->
    <div class="section">
      <div class="section-title">Processes</div>
      <n-spin :show="store.loading">
        <n-data-table
          :columns="processColumns"
          :data="store.processes"
          size="small"
          :single-line="false"
        />
      </n-spin>
    </div>

    <!-- R-14 §75：脚本执行确认管理（默认禁止自动执行；可重置） -->
    <div class="section">
      <div class="section-head">
        <div class="section-title">脚本执行确认（§75 Command Safety）</div>
        <div class="section-head-right">
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
        </div>
      </div>
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
    </div>

    <!-- Scheduler config (§66) -->
    <div class="section scheduler-section">
      <div class="section-title">Runtime Task Scheduler（§66 限流并发）</div>
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
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, NTag, NIcon, useMessage, useDialog } from "naive-ui";
import {
  RefreshOutline,
  PlayOutline,
  StopOutline,
} from "@vicons/ionicons5";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRuntimeStore } from "@/stores/runtime";
import * as runtimeApi from "@/api/runtime";
import type {
  RuntimeApplicationConfig,
  RuntimeConfigSummary,
  RuntimeProcessInfo,
  SchedulerConfig,
} from "@/types/runtime";
import type { RuntimeScope } from "@/types/maven";
import { errMsg } from "@/utils/error";
import RuntimeErrorAlert from "@/components/runtime/RuntimeErrorAlert.vue";
import type { ScriptApproval } from "@/types/runtime";

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const store = useRuntimeStore();
const message = useMessage();
const dialog = useDialog();

const resolving = ref(false);
const selectedConfig = ref<RuntimeConfigSummary | null>(null);
/** 选中应用的完整配置（按需加载）。 */
const configDetail = ref<RuntimeApplicationConfig | null>(null);
const scheduler = ref<SchedulerConfig>({ maxConcurrentBuilds: 2, maxConcurrentResolves: 4 });
const savingScheduler = ref(false);

// ------------------------------------------------------------------
// R-14 §80 可行动错误提示 + §75 脚本确认流
// ------------------------------------------------------------------

/** 最近一次操作的结构化错误（页面顶部横幅）。 */
const lastError = ref<unknown>(null);
/** 失败的操作名。 */
const failedAction = ref("");
/** 确认脚本后自动重试的操作。 */
const pendingRetry = ref<(() => Promise<void>) | null>(null);

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
    case "running":
      return store.health.get(name) === "down"
        ? { label: "Unhealthy", cls: "unhealthy" }
        : { label: "Running", cls: "running" };
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
const startingCount = computed(
  () => store.configs.filter((c) => {
    const p = processOf(c.name);
    return !!p && ["preparing", "resolving", "building", "starting"].includes(p.status);
  }).length,
);
const failedCount = computed(
  () => store.configs.filter((c) => processOf(c.name)?.status === "failed").length,
);

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
  const wsId = workspaceStore.currentWorkspace?.id;
  if (!wsId) return;
  await store.setWorkspace(wsId);
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
  await store.subscribe();
  await reload();
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
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 10px;
}
.stat-card {
  border: 1px solid var(--gw-border);
  border-radius: 8px;
  padding: 12px 14px;
  background: var(--gw-bg-panel);
}
.stat-label {
  font-size: 12px;
  color: var(--gw-text-dim);
}
.stat-value {
  font-size: 26px;
  font-weight: 600;
  line-height: 1.3;
}
.stat-sub {
  font-size: 12px;
  color: var(--gw-text-dim);
}
.tone-ok .stat-value {
  color: var(--gw-success);
}
.tone-warn .stat-value {
  color: var(--gw-warning);
}
.tone-danger .stat-value {
  color: var(--gw-danger);
}
.tone-info .stat-value {
  color: var(--gw-accent);
}
.section {
  border: 1px solid var(--gw-border);
  border-radius: 8px;
  padding: 12px 14px;
}
.section-head {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
}
.section-head-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
.section-title {
  font-size: 13px;
  font-weight: 600;
}
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
  background: #c0c4cc;
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
  background: #67c23a;
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
