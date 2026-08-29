<template>
  <div class="section">
    <n-alert type="info" :show-icon="false">
      默认模型解析顺序：<b>任务显式选择</b> &gt; <b>Workspace 任务配置</b> &gt;
      <b>全局任务默认</b> &gt; <b>全局聊天默认</b> &gt; <b>首个可用模型</b>。
      模型不具备任务所需能力（如结构化输出）时无法被设为默认。
    </n-alert>

    <!-- 全局任务默认 -->
    <div class="block">
      <div class="block-title">全局任务默认</div>
      <n-data-table
        :data="globalRows"
        :columns="globalColumns"
        :row-key="(r: TaskRow) => r.kind"
        empty-text="暂无默认模型：未配置时按「全局聊天默认 &gt; 首个可用模型」解析"
      />
    </div>

    <!-- Workspace 覆盖 -->
    <div class="block">
      <div class="block-title">Workspace 覆盖</div>
      <n-select
        v-model:value="selectedWorkspace"
        :options="workspaceOptions"
        placeholder="选择 Workspace 查看或配置覆盖（缺省继承全局）"
        clearable
        class="workspace-select"
      />
      <n-data-table
        v-if="selectedWorkspace !== null"
        :data="workspaceRows"
        :columns="workspaceColumns"
        :row-key="(r: TaskRow) => r.kind"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useMessage, NButton, NSelect, NTag } from "naive-ui";
import {
  aiClearTaskDefaultModel,
  aiSetTaskDefaultModel,
} from "@/api/ai";
import { useWorkspaceStore } from "@/stores/workspace";
import { errMsg } from "@/utils/error";
import type {
  AiModel,
  AiProvider,
  AiTaskDefault,
  AiTaskKind,
} from "@/types/ai";

const props = defineProps<{
  providers: AiProvider[];
  models: AiModel[];
  taskDefaults: AiTaskDefault[];
}>();
const emit = defineEmits<{ refresh: [] }>();

const message = useMessage();
const workspaceStore = useWorkspaceStore();

const taskKinds: { kind: AiTaskKind; label: string }[] = [
  { kind: "chat", label: "聊天（默认兜底）" },
  { kind: "runtimeDiagnostic", label: "Runtime 诊断" },
  { kind: "gitReview", label: "Git Review" },
  { kind: "commitMessage", label: "提交信息" },
  { kind: "conflict", label: "冲突解决" },
];

interface TaskRow {
  kind: AiTaskKind;
  label: string;
  /** 当前生效的 `providerId::modelId`；null = 未配置。 */
  value: string | null;
}

const modelOptions = computed(() => {
  const enabledProviders = new Set(
    props.providers.filter((p) => p.enabled).map((p) => p.id),
  );
  return props.models
    .filter((m) => m.enabled && enabledProviders.has(m.providerId))
    .map((m) => ({
      label: `${m.displayName}（${m.id} · ${providerName(m.providerId)}）`,
      value: `${m.providerId}::${m.id}`,
    }));
});

function providerName(id: string) {
  return props.providers.find((p) => p.id === id)?.name ?? id;
}

function defaultsFor(kind: AiTaskKind, workspaceId: number | null) {
  return props.taskDefaults.find(
    (d) => d.taskKind === kind && (d.workspaceId ?? null) === workspaceId,
  );
}

function rowFor(kind: AiTaskKind, label: string, workspaceId: number | null): TaskRow {
  const d = defaultsFor(kind, workspaceId);
  return {
    kind,
    label,
    value: d ? `${d.providerId}::${d.modelId}` : null,
  };
}

const globalRows = computed(() =>
  taskKinds.map(({ kind, label }) => rowFor(kind, label, null)),
);

// ---- Workspace 覆盖 ----
onMounted(() => {
  workspaceStore.loadWorkspaces();
});

const workspaceOptions = computed(() =>
  workspaceStore.workspaces.map((w) => ({ label: w.name, value: w.id })),
);

const selectedWorkspace = ref<number | null>(null);

const workspaceRows = computed(() => {
  if (selectedWorkspace.value === null) return [];
  return taskKinds.map(({ kind, label }) => rowFor(kind, label, selectedWorkspace.value));
});

/** 全局默认值选择/清除。 */
async function setGlobal(row: TaskRow, value: string | null) {
  try {
    if (value === null) {
      await aiClearTaskDefaultModel(row.kind);
      message.success(`已清除「${row.label}」默认模型`);
    } else {
      const [providerId, modelId] = value.split("::");
      await aiSetTaskDefaultModel(row.kind, providerId, modelId);
      message.success(`「${row.label}」默认模型已更新`);
    }
    emit("refresh");
  } catch (e) {
    message.error(errMsg(e));
    emit("refresh");
  }
}

/** Workspace 覆盖选择/清除（清除 = 继承全局）。 */
async function setWorkspaceOverride(row: TaskRow, value: string | null) {
  if (selectedWorkspace.value === null) return;
  try {
    if (value === null) {
      await aiClearTaskDefaultModel(row.kind, selectedWorkspace.value);
      message.success(`「${row.label}」已继承全局`);
    } else {
      const [providerId, modelId] = value.split("::");
      await aiSetTaskDefaultModel(row.kind, providerId, modelId, selectedWorkspace.value);
      message.success(`「${row.label}」Workspace 覆盖已保存`);
    }
    emit("refresh");
  } catch (e) {
    message.error(errMsg(e));
    emit("refresh");
  }
}

function sourceTag(kind: AiTaskKind) {
  // 有效来源链中该项是否有全局配置（无配置时运行时回落到聊天默认/首个可用）
  const d = defaultsFor(kind, null);
  if (!d) {
    return h(
      NTag,
      { size: "small", type: "warning", bordered: false },
      { default: () => "未配置（自动回落）" },
    );
  }
  return null;
}

const globalColumns = [
  { title: "任务", key: "label" },
  {
    title: "默认模型",
    key: "model",
    render: (row: TaskRow) =>
      h(NSelect, {
        value: row.value,
        options: modelOptions.value,
        placeholder: "未配置（自动回落）",
        clearable: true,
        filterable: true,
        onUpdateValue: (v: string | null) => setGlobal(row, v),
      }),
  },
  {
    title: "状态",
    key: "status",
    width: 170,
    render: (row: TaskRow) => sourceTag(row.kind),
  },
  {
    title: "操作",
    key: "actions",
    width: 90,
    render: (row: TaskRow) =>
      h(
        NButton,
        {
          size: "small",
          quaternary: true,
          disabled: row.value === null,
          onClick: () => setGlobal(row, null),
        },
        { default: () => "清除" },
      ),
  },
];

const workspaceColumns = [
  { title: "任务", key: "label" },
  {
    title: "Workspace 覆盖模型",
    key: "model",
    render: (row: TaskRow) =>
      h(NSelect, {
        value: row.value,
        options: modelOptions.value,
        placeholder: "继承全局",
        clearable: true,
        filterable: true,
        onUpdateValue: (v: string | null) => setWorkspaceOverride(row, v),
      }),
  },
];
</script>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-4);
}

.block {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.block-title {
  font-size: 13px;
  font-weight: 600;
}

.workspace-select {
  max-width: 360px;
}
</style>
