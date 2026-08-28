<template>
  <div class="environments-view">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button type="primary" :disabled="!workspaceId" @click="openEditor(null)">
          <template #icon><n-icon><AddOutline /></n-icon></template>
          新建环境
        </n-button>
        <n-button :loading="loading" :disabled="!workspaceId" @click="reload">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
      <div class="toolbar-right">
        <span class="hint">
          R-15 §38：按服务依赖拓扑排序启动——无依赖服务并行，依赖服务等待上游
          Healthy（R-16 就绪门限）。
        </span>
      </div>
    </div>

    <n-spin :show="loading">
      <div v-if="environments.length === 0 && !loading" class="empty">
        <n-empty description="暂无 Runtime 环境——点击「新建环境」定义服务集合与依赖关系" />
      </div>

      <Panel v-for="env in environments" :key="env.name" class="env-panel">
        <template #header>
          <span class="env-name">{{ env.name }}</span>
          <n-tag v-if="env.description" size="small" type="info">{{ env.description }}</n-tag>
        </template>
        <template #header-extra>
          <n-space>
            <n-button
              type="success"
              size="small"
              :loading="startingEnv === env.name"
              :disabled="workspaceId == null"
              @click="onStartEnv(env.name)"
            >
              <template #icon><n-icon><PlayOutline /></n-icon></template>
              启动环境
            </n-button>
            <n-button
              type="error"
              size="small"
              :loading="stoppingEnv === env.name"
              :disabled="workspaceId == null"
              @click="onStopEnv(env.name)"
            >
              <template #icon><n-icon><StopOutline /></n-icon></template>
              停止环境
            </n-button>
            <n-button size="small" @click="openEditor(env)">编辑</n-button>
            <n-popconfirm @positive-click="onDelete(env.name)">
              <template #trigger>
                <n-button size="small" quaternary type="error">删除</n-button>
              </template>
              删除环境「{{ env.name }}」？（只删环境定义，不影响 Runtime 配置）
            </n-popconfirm>
          </n-space>
        </template>

        <n-data-table
          :columns="serviceColumns"
          :data="env.services"
          :row-key="(row: EnvironmentService) => row.runtimeName"
          size="small"
          :single-line="false"
        />

        <!-- 实时编排进度（environment_progress 事件驱动） -->
        <div v-if="Object.keys(progressOf(env.name)).length" class="progress-box">
          <div class="progress-title">最近一次编排</div>
          <div
            v-for="(state, service) in progressOf(env.name)"
            :key="service"
            class="progress-row"
          >
            <n-tag :type="stateTagType(state.state)" size="small">
              {{ stateLabel(state.state) }}
            </n-tag>
            <span class="mono service-name">{{ service }}</span>
            <span class="detail">{{ state.detail ?? "" }}</span>
          </div>
        </div>
      </Panel>
    </n-spin>

    <!-- 编辑器 -->
    <n-modal
      v-model:show="editorShow"
      preset="card"
      :title="editing ? `编辑环境 · ${editing.name}` : '新建环境'"
      :style="{ width: '720px' }"
    >
      <n-space vertical :size="12">
        <n-form-item label="环境名称" :show-feedback="false">
          <n-input
            v-model:value="form.name"
            :disabled="!!editing"
            placeholder="如 Development / Test / Demo"
          />
        </n-form-item>
        <n-form-item label="描述" :show-feedback="false">
          <n-input v-model:value="form.description" placeholder="可选" />
        </n-form-item>

        <div class="section-title">服务列表</div>
        <n-data-table
          :columns="editColumns"
          :data="form.services"
          :row-key="(row: EnvironmentService) => row.runtimeName"
          size="small"
          :single-line="false"
        />
        <n-space>
          <n-select
            v-model:value="newServiceName"
            :options="runtimeOptions"
            placeholder="选择要加入的 Runtime 配置"
            style="width: 320px"
            filterable
          />
          <n-button :disabled="!newServiceName" @click="addService">添加服务</n-button>
        </n-space>
        <n-alert v-if="form.services.length" type="info" :show-icon="false">
          依赖关系决定启动顺序：启动按拓扑序分波（波内并行），停止按逆序。环依赖会在保存时被拒绝。
        </n-alert>
      </n-space>
      <template #footer>
        <n-space justify="end">
          <n-button @click="editorShow = false">取消</n-button>
          <n-button type="primary" :loading="saving" @click="onSave">保存</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
// R-15 §38/§39/§40 UI：环境列表 + 编辑器（服务/依赖/覆盖项）+ 一键启停 +
// 实时编排进度（environment_progress / environment_completed 事件驱动）。
import { computed, h, onMounted, onUnmounted, reactive, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  NAlert,
  NButton,
  NEmpty,
  NFormItem,
  NIcon,
  NInput,
  NInputNumber,
  NModal,
  NPopconfirm,
  NSelect,
  NSpace,
  NSpin,
  NTag,
  useMessage,
} from "naive-ui";
import { AddOutline, PlayOutline, RefreshOutline, StopOutline } from "@vicons/ionicons5";
import Panel from "@/components/shell/Panel.vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRuntimeStore } from "@/stores/runtime";
import {
  RUNTIME_EVENTS,
  runtimeDeleteEnvironment,
  runtimeListEnvironments,
  runtimeSaveEnvironment,
  runtimeStartNamedEnvironment,
  runtimeStopNamedEnvironment,
} from "@/api/runtime";
import type {
  EnvironmentProgressPayload,
  EnvironmentService,
  RuntimeEnvironment,
  ServiceExecState,
} from "@/types/runtime";
import { errMsg } from "@/utils/error";

const message = useMessage();
const workspaceStore = useWorkspaceStore();
const runtimeStore = useRuntimeStore();

const workspaceId = computed(() => workspaceStore.currentWorkspace?.id ?? null);
const environments = ref<RuntimeEnvironment[]>([]);
const loading = ref(false);
const startingEnv = ref<string | null>(null);
const stoppingEnv = ref<string | null>(null);
const saving = ref(false);

/** envName → service → 最新编排状态。 */
const progress = ref<Map<string, Map<string, { state: ServiceExecState; detail: string | null }>>>(
  new Map(),
);

let unlisteners: UnlistenFn[] = [];

// ------------------------------------------------------------------
// 加载 + 事件订阅
// ------------------------------------------------------------------

async function reload() {
  if (workspaceId.value == null) {
    environments.value = [];
    return;
  }
  loading.value = true;
  try {
    environments.value = await runtimeListEnvironments(workspaceId.value);
  } catch (e) {
    message.error("环境列表加载失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function progressOf(envName: string): Record<string, { state: ServiceExecState; detail: string | null }> {
  return Object.fromEntries(progress.value.get(envName) ?? new Map());
}

function stateLabel(state: ServiceExecState): string {
  const map: Record<ServiceExecState, string> = {
    skipped: "Skipped",
    starting: "Starting",
    ready: "Ready",
    failed: "Failed",
    stopped: "Stopped",
  };
  return map[state];
}

function stateTagType(state: ServiceExecState): "success" | "error" | "warning" | "info" | "default" {
  switch (state) {
    case "ready":
      return "success";
    case "failed":
      return "error";
    case "starting":
      return "info";
    case "skipped":
      return "warning";
    default:
      return "default";
  }
}

// ------------------------------------------------------------------
// 启停
// ------------------------------------------------------------------

async function onStartEnv(name: string) {
  if (workspaceId.value == null) return;
  startingEnv.value = name;
  progress.value.set(name, new Map());
  try {
    await runtimeStartNamedEnvironment(workspaceId.value, name);
    message.success(`环境「${name}」启动任务已提交，进度见下方实时编排状态`);
  } catch (e) {
    message.error("启动环境失败：" + errMsg(e));
  } finally {
    startingEnv.value = null;
  }
}

async function onStopEnv(name: string) {
  if (workspaceId.value == null) return;
  stoppingEnv.value = name;
  try {
    await runtimeStopNamedEnvironment(workspaceId.value, name);
    message.success(`环境「${name}」停止任务已提交`);
  } catch (e) {
    message.error("停止环境失败：" + errMsg(e));
  } finally {
    stoppingEnv.value = null;
  }
}

async function onDelete(name: string) {
  if (workspaceId.value == null) return;
  try {
    await runtimeDeleteEnvironment(workspaceId.value, name);
    message.success(`环境「${name}」已删除`);
    await reload();
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
  }
}

// ------------------------------------------------------------------
// 编辑器
// ------------------------------------------------------------------

const editorShow = ref(false);
const editing = ref<RuntimeEnvironment | null>(null);
const newServiceName = ref<string | null>(null);
const form = reactive<{ name: string; description: string; services: EnvironmentService[] }>({
  name: "",
  description: "",
  services: [],
});

const runtimeOptions = computed(() =>
  runtimeStore.configs.map((c) => ({ label: c.name, value: c.name })),
);

function openEditor(env: RuntimeEnvironment | null) {
  editing.value = env;
  form.name = env?.name ?? "";
  form.description = env?.description ?? "";
  form.services = env ? JSON.parse(JSON.stringify(env.services)) : [];
  editorShow.value = true;
}

function addService() {
  if (!newServiceName.value) return;
  if (form.services.some((s) => s.runtimeName === newServiceName.value)) {
    message.warning("该服务已在列表中");
    return;
  }
  form.services.push({
    runtimeName: newServiceName.value,
    dependsOn: [],
    jdk: null,
    profile: null,
    environment: {},
    port: null,
    externalNotes: null,
    readyTimeoutSeconds: null,
  });
  newServiceName.value = null;
}

function removeService(index: number) {
  const removed = form.services[index].runtimeName;
  form.services.splice(index, 1);
  // 清理指向被删服务的依赖。
  for (const s of form.services) {
    s.dependsOn = s.dependsOn.filter((d) => d !== removed);
  }
}

const serviceOptionsFor = (exclude: string) =>
  form.services
    .filter((s) => s.runtimeName !== exclude)
    .map((s) => ({ label: s.runtimeName, value: s.runtimeName }));

const editColumns = [
  {
    title: "服务（Runtime 配置）",
    key: "runtimeName",
    minWidth: 140,
    render: (row: EnvironmentService) => h("span", { class: "mono" }, row.runtimeName),
  },
  {
    title: "依赖（启动前置）",
    key: "dependsOn",
    minWidth: 220,
    render: (row: EnvironmentService) =>
      h(NSelect, {
        value: row.dependsOn,
        multiple: true,
        clearable: true,
        size: "small",
        options: serviceOptionsFor(row.runtimeName),
        placeholder: "无依赖（第一波启动）",
        onUpdateValue: (v: string[]) => {
          row.dependsOn = v;
        },
      }),
  },
  {
    title: "端口覆盖",
    key: "port",
    width: 130,
    render: (row: EnvironmentService) =>
      h(NInputNumber, {
        value: row.port,
        size: "small",
        min: 1,
        max: 65535,
        showButton: false,
        placeholder: "跟随配置",
        onUpdateValue: (v: number | null) => {
          row.port = v;
        },
      }),
  },
  {
    title: "外部服务备注",
    key: "externalNotes",
    minWidth: 160,
    render: (row: EnvironmentService) =>
      h(NInput, {
        value: row.externalNotes ?? "",
        size: "small",
        placeholder: "如：依赖外部 MySQL",
        onUpdateValue: (v: string) => {
          row.externalNotes = v || null;
        },
      }),
  },
  {
    title: "",
    key: "actions",
    width: 60,
    render: (_row: EnvironmentService, index: number) =>
      h(
        NButton,
        { size: "small", text: true, type: "error", onClick: () => removeService(index) },
        { default: () => "移除" },
      ),
  },
];

const serviceColumns = [
  {
    title: "服务",
    key: "runtimeName",
    minWidth: 140,
    render: (row: EnvironmentService) => h("span", { class: "mono" }, row.runtimeName),
  },
  {
    title: "依赖",
    key: "dependsOn",
    render: (row: EnvironmentService) =>
      row.dependsOn.length ? row.dependsOn.join(" ← ") : "—（第一波并行启动）",
  },
  {
    title: "JDK",
    key: "jdk",
    width: 80,
    render: (row: EnvironmentService) => row.jdk ?? "跟随配置",
  },
  {
    title: "Profile",
    key: "profile",
    width: 90,
    render: (row: EnvironmentService) => row.profile ?? "跟随配置",
  },
  {
    title: "端口",
    key: "port",
    width: 80,
    render: (row: EnvironmentService) => (row.port != null ? String(row.port) : "跟随配置"),
  },
  {
    title: "外部服务备注",
    key: "externalNotes",
    render: (row: EnvironmentService) => row.externalNotes ?? "—",
  },
];

async function onSave() {
  if (workspaceId.value == null) return;
  if (!form.name.trim()) {
    message.warning("请填写环境名称");
    return;
  }
  const environment: RuntimeEnvironment = {
    schemaVersion: 1,
    name: form.name.trim(),
    description: form.description.trim() || null,
    services: form.services,
  };
  saving.value = true;
  try {
    await runtimeSaveEnvironment(workspaceId.value, environment);
    message.success(`环境「${environment.name}」已保存（可提交到 Git 团队共享）`);
    editorShow.value = false;
    await reload();
  } catch (e) {
    message.error("保存失败：" + errMsg(e));
  } finally {
    saving.value = false;
  }
}

// ------------------------------------------------------------------
// 生命周期
// ------------------------------------------------------------------

onMounted(async () => {
  await reload();
  unlisteners.push(
    await listen<EnvironmentProgressPayload>(RUNTIME_EVENTS.environmentProgress, (e) => {
      const payload = e.payload;
      let services = progress.value.get(payload.environment);
      if (!services) {
        services = new Map();
        progress.value.set(payload.environment, services);
      }
      services.set(payload.service, { state: payload.state, detail: payload.detail });
    }),
    await listen<{ environment: string }>(RUNTIME_EVENTS.environmentCompleted, (e) => {
      message.info(`环境「${e.payload.environment}」编排结束`);
    }),
  );
});

onUnmounted(() => {
  for (const un of unlisteners) un();
  unlisteners = [];
});
</script>

<style scoped>
.environments-view {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3, 12px);
  padding: var(--gw-space-3, 12px) var(--gw-space-4, 16px);
  height: 100%;
  overflow-y: auto;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2, 8px);
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: var(--gw-space-2, 8px);
  align-items: center;
}
.hint {
  font-size: var(--gw-text-xs, 12px);
  color: var(--gw-text-dim, #999);
}
.empty {
  padding: 48px 0;
}
.env-panel {
  margin-bottom: var(--gw-space-3, 12px);
}
.env-name {
  font-weight: 600;
}
.mono {
  font-family: var(--gw-font-mono, monospace);
}
.progress-box {
  margin-top: var(--gw-space-2, 8px);
  padding: var(--gw-space-2, 8px);
  border: 1px solid var(--gw-border, #333);
  border-radius: var(--gw-radius-sm, 6px);
}
.progress-title {
  font-size: var(--gw-text-xs, 12px);
  color: var(--gw-text-dim, #999);
  margin-bottom: 4px;
}
.progress-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2, 8px);
  padding: 2px 0;
}
.service-name {
  min-width: 120px;
}
.detail {
  font-size: var(--gw-text-xs, 12px);
  color: var(--gw-text-dim, #999);
}
.section-title {
  font-weight: 600;
}
</style>
