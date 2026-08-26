<template>
  <div class="runtime-dashboard">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          @change="onWorkspaceChange"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-button :loading="store.loading" @click="reload">
          <el-icon><RefreshRight /></el-icon>
          刷新
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          :loading="resolving"
          @click="onResolve"
        >
          <el-icon><Refresh /></el-icon>
          解析依赖
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="router.push({ name: 'runtime-dependencies' })"
        >
          <el-icon><Share /></el-icon>
          依赖映射
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="router.push({ name: 'runtime-scope' })"
        >
          <el-icon><SetUp /></el-icon>
          Scope
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="router.push({ name: 'runtime-logs' })"
        >
          <el-icon><Document /></el-icon>
          日志
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="router.push({ name: 'runtime-app-wizard' })"
        >
          <el-icon><Plus /></el-icon>
          新建应用
        </el-button>
        <el-button
          type="success"
          plain
          :disabled="!selectedWorkspaceId || store.configs.length === 0"
          @click="onStartAll"
        >
          <el-icon><VideoPlay /></el-icon>
          全部启动
        </el-button>
        <el-button
          type="danger"
          plain
          :disabled="!selectedWorkspaceId || store.processes.length === 0"
          @click="onStopAll"
        >
          <el-icon><VideoPause /></el-icon>
          全部停止
        </el-button>
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
    <div class="cards" v-loading="store.loading">
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
      <el-table
        :data="store.configs"
        v-loading="store.loading"
        empty-text="暂无 Runtime 应用配置"
        row-key="id"
        highlight-current-row
        @current-change="onSelectRow"
      >
        <el-table-column label="状态" width="130">
          <template #default="{ row }">
            <span class="status-cell">
              <span class="status-dot" :class="statusOf(row.name).cls"></span>
              {{ statusOf(row.name).label }}
            </span>
            <div v-if="stageOf(row.name)" class="stage-text">
              {{ stageOf(row.name) }}
            </div>
          </template>
        </el-table-column>
        <el-table-column prop="name" label="名称" min-width="120">
          <template #default="{ row }">
            <span class="app-name">{{ row.name }}</span>
          </template>
        </el-table-column>
        <el-table-column prop="project" label="项目" min-width="180" show-overflow-tooltip>
          <template #default="{ row }">
            <span class="mono">{{ row.project }}</span>
          </template>
        </el-table-column>
        <el-table-column label="Main Class" min-width="200" show-overflow-tooltip>
          <template #default="{ row }">
            <span v-if="row.mainClass" class="mono">{{ row.mainClass }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="JDK" width="70">
          <template #default="{ row }">
            <span v-if="row.jdk" class="mono">{{ row.jdk }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="Profile" width="90">
          <template #default="{ row }">
            <el-tag v-if="row.profile" size="small" type="warning" effect="plain">
              {{ row.profile }}
            </el-tag>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="PID" width="80">
          <template #default="{ row }">
            <span v-if="processOf(row.name)?.pid" class="mono">
              {{ processOf(row.name)!.pid }}
            </span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="端口" width="90">
          <template #default="{ row }">
            <span v-if="processOf(row.name)?.ports?.length" class="mono">
              {{ processOf(row.name)!.ports.join(",") }}
            </span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="330" fixed="right">
          <template #default="{ row }">
            <el-button
              size="small"
              type="primary"
              :disabled="isBusy(row.name)"
              @click="onStart(row.name)"
            >
              启动
            </el-button>
            <el-button
              size="small"
              :disabled="!isRunning(row.name)"
              @click="onStop(row.name)"
            >
              停止
            </el-button>
            <el-button
              size="small"
              :disabled="!isRunning(row.name)"
              @click="onRestart(row.name)"
            >
              重启
            </el-button>
            <el-button
              size="small"
              plain
              :disabled="isBusy(row.name)"
              @click="onBuild(row.name)"
            >
              构建
            </el-button>
            <el-button
              size="small"
              link
              type="primary"
              @click="openLogs(row.name)"
            >
              日志
            </el-button>
            <el-button
              size="small"
              link
              @click="openWizard(row.name)"
            >
              配置
            </el-button>
            <el-popconfirm
              title="确定删除该 Runtime 配置吗？"
              confirm-button-text="删除"
              cancel-button-text="取消"
              @confirm="onDelete(row as RuntimeConfigSummary)"
            >
              <template #reference>
                <el-button size="small" link type="danger">删除</el-button>
              </template>
            </el-popconfirm>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- Selected application detail -->
    <div v-if="selectedConfig" class="section">
      <div class="section-title">
        应用详情 · {{ selectedConfig.name }}
        <el-tag v-if="configDetail" size="small" class="scope-tag" type="info" effect="plain">
          Scope: {{ scopeLabel(configDetail.scope) }}
        </el-tag>
      </div>
      <el-descriptions :column="4" border size="small">
        <el-descriptions-item label="JDK">
          {{ configDetail?.jdk ?? selectedConfig.jdk ?? "—" }}
        </el-descriptions-item>
        <el-descriptions-item label="Profile">
          {{ configDetail?.profile ?? selectedConfig.profile ?? "—" }}
        </el-descriptions-item>
        <el-descriptions-item label="PID">
          {{ processOf(selectedConfig.name)?.pid ?? "—" }}
        </el-descriptions-item>
        <el-descriptions-item label="端口">
          {{ processOf(selectedConfig.name)?.ports?.join(", ") || "—" }}
        </el-descriptions-item>
        <el-descriptions-item label="内存" :span="1">
          {{
            processOf(selectedConfig.name)?.memoryBytes
              ? formatBytes(processOf(selectedConfig.name)!.memoryBytes!)
              : "—"
          }}
        </el-descriptions-item>
        <el-descriptions-item label="CPU" :span="1">
          {{
            processOf(selectedConfig.name)?.cpuPercent != null
              ? processOf(selectedConfig.name)!.cpuPercent!.toFixed(1) + "%"
              : "—"
          }}
        </el-descriptions-item>
        <el-descriptions-item label="运行策略" :span="2">
          {{ processOf(selectedConfig.name)?.runStrategy ?? "—" }}
        </el-descriptions-item>
        <el-descriptions-item label="VM Options" :span="4">
          <span v-if="configDetail?.vmOptions?.length" class="mono">{{
            configDetail.vmOptions.join(" ")
          }}</span>
          <span v-else class="muted">—</span>
        </el-descriptions-item>
        <el-descriptions-item label="Program Args" :span="4">
          <span v-if="configDetail?.programArguments?.length" class="mono">{{
            configDetail.programArguments.join(" ")
          }}</span>
          <span v-else class="muted">—</span>
        </el-descriptions-item>
        <el-descriptions-item label="启动命令预览" :span="4">
          <span v-if="processOf(selectedConfig.name)?.commandPreview" class="mono cmd-preview">
            {{ processOf(selectedConfig.name)!.commandPreview }}
          </span>
          <span v-else class="muted">—</span>
        </el-descriptions-item>
        <el-descriptions-item label="环境变量" :span="4">
          <span v-if="configDetail && envKeys(configDetail).length > 0" class="mono env-summary">
            {{ envKeys(configDetail).join(", ") }}
          </span>
          <span v-else class="muted">—</span>
        </el-descriptions-item>
      </el-descriptions>
    </div>

    <!-- Processes -->
    <div class="section">
      <div class="section-title">Processes</div>
      <el-table
        :data="store.processes"
        v-loading="store.loading"
        empty-text="暂无进程记录"
        size="small"
      >
        <el-table-column prop="processId" label="ID" width="70" />
        <el-table-column prop="runtimeName" label="Runtime" min-width="120" />
        <el-table-column label="PID" width="90">
          <template #default="{ row }">
            <span class="mono">{{ row.pid ?? "—" }}</span>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="110">
          <template #default="{ row }">
            <span class="status-cell">
              <span class="status-dot" :class="statusOf(row.runtimeName).cls"></span>
              {{ statusOf(row.runtimeName).label }}
            </span>
          </template>
        </el-table-column>
        <el-table-column label="策略" width="110">
          <template #default="{ row }">
            <el-tag v-if="row.runStrategy" size="small" effect="plain">
              {{ row.runStrategy }}
            </el-tag>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="端口" width="110">
          <template #default="{ row }">
            <span class="mono">{{ row.ports?.join(", ") || "—" }}</span>
          </template>
        </el-table-column>
        <el-table-column label="已运行" width="110">
          <template #default="{ row }">
            <span v-if="row.uptimeSeconds != null">{{ formatUptime(row.uptimeSeconds) }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="CPU" width="80">
          <template #default="{ row }">
            <span v-if="row.cpuPercent != null">{{ row.cpuPercent.toFixed(1) }}%</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="内存" width="100">
          <template #default="{ row }">
            <span v-if="row.memoryBytes != null">{{ formatBytes(row.memoryBytes) }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="退出码" width="80">
          <template #default="{ row }">
            <span v-if="row.exitCode != null" class="mono">{{ row.exitCode }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="孤儿接管" width="90">
          <template #default="{ row }">
            <el-tag v-if="row.adopted" size="small" type="warning">已接管</el-tag>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="启动于" width="160">
          <template #default="{ row }">
            <span class="muted">{{ formatTime(row.startedAt) }}</span>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <!-- R-14 §75：脚本执行确认管理（默认禁止自动执行；可重置） -->
    <div class="section">
      <div class="section-head">
        <div class="section-title">脚本执行确认（§75 Command Safety）</div>
        <div class="section-head-right">
          <el-button size="small" :loading="approvalsLoading" @click="loadApprovals">
            <el-icon><Refresh /></el-icon>
            刷新
          </el-button>
          <el-popconfirm
            title="撤销当前 workspace 的全部脚本确认？"
            confirm-button-text="撤销"
            cancel-button-text="取消"
            @confirm="onResetApprovals(store.workspaceId, null)"
          >
            <template #reference>
              <el-button
                size="small"
                type="danger"
                plain
                :disabled="workspaceApprovals.length === 0"
              >
                全部重置
              </el-button>
            </template>
          </el-popconfirm>
        </div>
      </div>
      <el-table
        :data="workspaceApprovals"
        size="small"
        v-loading="approvalsLoading"
        empty-text="当前 workspace 暂无已确认的脚本（脚本首次执行时会要求确认）"
      >
        <el-table-column label="Runtime" min-width="120">
          <template #default="{ row }">
            <span class="app-name">{{ row.runtimeName }}</span>
          </template>
        </el-table-column>
        <el-table-column label="类型" width="110">
          <template #default="{ row }">
            <el-tag size="small" :type="row.scriptType === 'pre' ? 'warning' : 'success'" effect="plain">
              {{ row.scriptType === "pre" ? "Pre-Build" : "Post-Build" }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="脚本预览" min-width="220" show-overflow-tooltip>
          <template #default="{ row }">
            <span class="mono">{{ row.preview }}</span>
          </template>
        </el-table-column>
        <el-table-column label="确认于" width="160">
          <template #default="{ row }">
            <span class="muted">{{ formatTime(row.approvedAt) }}</span>
          </template>
        </el-table-column>
        <el-table-column label="最近执行" width="160">
          <template #default="{ row }">
            <span v-if="row.lastExecutedAt" class="muted">{{ formatTime(row.lastExecutedAt) }}</span>
            <span v-else class="muted">—</span>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="90">
          <template #default="{ row }">
            <el-button
              size="small"
              link
              type="danger"
              @click="onResetApprovals(row.workspaceId, row.runtimeName)"
            >
              重置
            </el-button>
          </template>
        </el-table-column>
      </el-table>
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
          <el-input-number
            v-model="scheduler.maxConcurrentBuilds"
            :min="1"
            :max="16"
            size="small"
          />
        </div>
        <div class="scheduler-field">
          <span class="scheduler-label">最大并发 Resolve</span>
          <el-input-number
            v-model="scheduler.maxConcurrentResolves"
            :min="1"
            :max="16"
            size="small"
          />
        </div>
        <el-button size="small" type="primary" :loading="savingScheduler" @click="onSaveScheduler">
          保存并生效
        </el-button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  Back,
  Refresh,
  RefreshRight,
  Plus,
  Share,
  SetUp,
  Document,
  VideoPlay,
  VideoPause,
} from "@element-plus/icons-vue";
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

const selectedWorkspaceId = ref<number | null>(null);
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
  ElMessage.error(`${action}失败：${errMsg(e)}`);
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
    ElMessage.error("缺少脚本确认所需信息");
    return;
  }
  try {
    await ElMessageBox.confirm(
      `Runtime「${runtimeName}」的 ${scriptType === "pre" ? "Pre-Build" : "Post-Build"} 脚本需要确认：\n\n${preview}\n\n确认后立即执行并记录；可随时在下方「脚本执行确认」中重置。`,
      "确认执行脚本",
      {
        confirmButtonText: "确认并执行",
        cancelButtonText: "取消",
        type: "warning",
      },
    );
  } catch {
    return; // 用户取消
  }
  try {
    await runtimeApi.runtimeApproveScript(workspaceId, runtimeName, scriptType);
    ElMessage.success("已确认脚本，正在重试操作…");
    await loadApprovals();
    const retry = pendingRetry.value;
    clearError();
    if (retry) {
      await retry();
    }
  } catch (e) {
    ElMessage.error("确认脚本失败：" + errMsg(e));
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
    ElMessage.success(`已撤销 ${removed} 条脚本确认`);
    await loadApprovals();
  } catch (e) {
    ElMessage.error("重置失败：" + errMsg(e));
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
// 操作
// ------------------------------------------------------------------

async function onStart(name: string) {
  clearError();
  try {
    await store.start(name);
    ElMessage.success(`已提交启动任务：${name}`);
  } catch (e) {
    handleError("启动", e, () => onStart(name));
  }
}

async function onStop(name: string) {
  clearError();
  try {
    await store.stop(name);
    ElMessage.success(`已提交停止任务：${name}`);
  } catch (e) {
    handleError("停止", e);
  }
}

async function onRestart(name: string) {
  clearError();
  try {
    await store.restart(name);
    ElMessage.success(`已提交重启任务：${name}`);
  } catch (e) {
    handleError("重启", e, () => onRestart(name));
  }
}

async function onBuild(name: string) {
  clearError();
  try {
    await store.build(name);
    ElMessage.success(`已提交构建任务：${name}`);
  } catch (e) {
    handleError("构建", e, () => onBuild(name));
  }
}

async function onResolve() {
  if (!selectedWorkspaceId.value) return;
  clearError();
  resolving.value = true;
  try {
    const taskId = await store.resolveDependencies();
    ElMessage.success(`依赖解析任务已提交：${taskId}`);
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
    ElMessage.success(`已提交 ${ids.length} 个启动任务`);
  } catch (e) {
    handleError("全部启动", e, () => onStartAll());
  }
}

async function onStopAll() {
  clearError();
  try {
    const ids = await store.stopEnvironment();
    ElMessage.success(`已提交 ${ids.length} 个停止任务`);
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
    ElMessage.success(`已删除配置：${row.name}`);
  } catch (e) {
    ElMessage.error("删除失败：" + errMsg(e));
  }
}

function onSelectRow(row: RuntimeConfigSummary | null) {
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
    ElMessage.success("调度并发上限已生效");
  } catch (e) {
    ElMessage.error("保存失败：" + errMsg(e));
  } finally {
    savingScheduler.value = false;
  }
}

// ------------------------------------------------------------------
// 数据加载
// ------------------------------------------------------------------

async function reload() {
  if (!selectedWorkspaceId.value) return;
  await store.setWorkspace(selectedWorkspaceId.value);
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

async function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  const ws = workspaceStore.workspaces.find((w) => w.id === id);
  if (ws) workspaceStore.selectWorkspace(ws);
  selectedConfig.value = null;
  await reload();
}

function goBack() {
  router.push({ name: "dashboard" });
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
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
  } else if (workspaceStore.workspaces.length > 0) {
    selectedWorkspaceId.value = workspaceStore.workspaces[0].id;
  }
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
  padding: 12px 16px;
  gap: 12px;
  overflow-y: auto;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 10px;
}
.stat-card {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px 14px;
  background: var(--el-bg-color);
}
.stat-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.stat-value {
  font-size: 26px;
  font-weight: 600;
  line-height: 1.3;
}
.stat-sub {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.tone-ok .stat-value {
  color: var(--el-color-success);
}
.tone-warn .stat-value {
  color: var(--el-color-warning);
}
.tone-danger .stat-value {
  color: var(--el-color-danger);
}
.tone-info .stat-value {
  color: var(--el-color-primary);
}
.section {
  border: 1px solid var(--el-border-color);
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
  gap: 8px;
  align-items: center;
}
.section-title {
  font-size: 13px;
  font-weight: 600;
}
.section-hint {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.warn-hint {
  color: var(--el-color-warning);
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
  background: #409eff;
}
.status-dot.building {
  background: #e6a23c;
}
.status-dot.running {
  background: #67c23a;
}
.status-dot.unhealthy {
  background: #f56c6c;
}
.status-dot.failed {
  background: #f56c6c;
}
.status-dot.other {
  background: #909399;
}
.stage-text {
  font-size: 11px;
  color: var(--el-color-primary);
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
  color: var(--el-text-color-secondary);
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
  gap: 8px;
}
.scheduler-label {
  font-size: 13px;
  color: var(--el-text-color-regular);
}
</style>
