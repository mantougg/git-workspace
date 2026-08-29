<template>
  <n-modal
    v-model:show="visible"
    preset="card"
    :title="`AI 任务默认模型 — ${workspace?.name ?? ''}`"
    style="width: 560px"
  >
    <n-alert type="info" :show-icon="false" class="hint">
      覆盖仅对该 Workspace 生效；选择「继承全局」时回落全局任务默认链
      （全局任务默认 &gt; 全局聊天默认 &gt; 首个可用模型）。
    </n-alert>

    <n-spin :show="loading">
      <div class="rows">
        <div v-for="task in taskKinds" :key="task.kind" class="row">
          <span class="row-label">{{ task.label }}</span>
          <n-select
            :value="currentValue(task.kind)"
            :options="selectOptions"
            placeholder="继承全局"
            clearable
            filterable
            size="small"
            class="row-select"
            @update:value="(v: string | null) => update(task.kind, v)"
          />
        </div>
      </div>
    </n-spin>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useMessage } from "naive-ui";
import {
  aiClearTaskDefaultModel,
  aiGetSettingsSummary,
  aiListModels,
  aiListProviders,
  aiSetTaskDefaultModel,
} from "@/api/ai";
import { errMsg } from "@/utils/error";
import type { AiModel, AiProvider, AiTaskDefault, AiTaskKind } from "@/types/ai";
import type { Workspace } from "@/types/workspace";

const props = defineProps<{ workspace: Workspace | null }>();

const visible = defineModel<boolean>({ default: false });

const message = useMessage();

const taskKinds: { kind: AiTaskKind; label: string }[] = [
  { kind: "chat", label: "聊天（默认兜底）" },
  { kind: "runtimeDiagnostic", label: "Runtime 诊断" },
  { kind: "gitReview", label: "Git Review" },
  { kind: "commitMessage", label: "提交信息" },
  { kind: "conflict", label: "冲突解决" },
];

const loading = ref(false);
const providers = ref<AiProvider[]>([]);
const models = ref<AiModel[]>([]);
const defaults = ref<AiTaskDefault[]>([]);

const selectOptions = computed(() => {
  const enabled = new Set(providers.value.filter((p) => p.enabled).map((p) => p.id));
  return models.value
    .filter((m) => m.enabled && enabled.has(m.providerId))
    .map((m) => ({
      label: `${m.displayName}（${m.id}）`,
      value: `${m.providerId}::${m.id}`,
    }));
});

function currentValue(kind: AiTaskKind): string | null {
  const wsId = props.workspace?.id;
  if (wsId == null) return null;
  const d = defaults.value.find(
    (x) => x.taskKind === kind && x.workspaceId === wsId,
  );
  return d ? `${d.providerId}::${d.modelId}` : null;
}

async function load() {
  if (!props.workspace) return;
  loading.value = true;
  try {
    const [p, m, s] = await Promise.all([
      aiListProviders(),
      aiListModels(),
      aiGetSettingsSummary(),
    ]);
    providers.value = p;
    models.value = m;
    defaults.value = s.taskDefaults;
  } catch (e) {
    message.error("加载 AI 配置失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

watch(
  () => [visible.value, props.workspace?.id],
  ([show]) => {
    if (show) load();
  },
);

async function update(kind: AiTaskKind, value: string | null) {
  const wsId = props.workspace?.id;
  if (wsId == null) return;
  try {
    if (value === null) {
      await aiClearTaskDefaultModel(kind, wsId);
    } else {
      const [providerId, modelId] = value.split("::");
      await aiSetTaskDefaultModel(kind, providerId, modelId, wsId);
    }
    // 重新拉取以反映后端校验（能力不匹配会被拒绝）后的实际状态。
    const s = await aiGetSettingsSummary();
    defaults.value = s.taskDefaults;
  } catch (e) {
    message.error(errMsg(e));
    load();
  }
}
</script>

<style scoped>
.hint {
  margin-bottom: var(--gw-space-3);
}

.rows {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}

.row-label {
  width: 150px;
  flex-shrink: 0;
  font-size: 13px;
}

.row-select {
  flex: 1;
}
</style>
