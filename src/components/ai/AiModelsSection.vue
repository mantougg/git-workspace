<template>
  <div class="section">
    <div class="section-toolbar">
      <n-select
        v-model:value="providerFilter"
        :options="filterOptions"
        clearable
        placeholder="按 Provider 筛选"
        class="provider-filter"
      />
      <n-button type="primary" :disabled="providers.length === 0" @click="openCreate">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        新增模型
      </n-button>
    </div>

    <n-alert v-if="providers.length === 0" type="info" :show-icon="false">
      先在「Provider」区块添加一个 Provider，再为其配置模型。
    </n-alert>

    <n-data-table
      :data="filteredModels"
      :columns="columns"
      :row-key="(m: AiModel) => `${m.providerId}::${m.id}`"
      empty-text="暂无模型，点击「新增模型」为 Provider 配置模型能力目录"
    />

    <!-- 新增/编辑模型 -->
    <n-modal
      v-model:show="form.show"
      preset="card"
      :title="form.editing ? '编辑模型' : '新增模型'"
      style="width: 560px"
    >
      <n-form label-width="110px">
        <n-form-item label="Provider" required>
          <n-select
            v-model:value="form.providerId"
            :options="providerOptions"
            :disabled="form.editing"
          />
        </n-form-item>
        <n-form-item label="模型 ID" required>
          <n-input
            v-model:value="form.id"
            placeholder="如 gpt-4o-mini（Provider 侧的模型名）"
            :input-props="{ spellcheck: false }"
            :disabled="form.editing"
          />
        </n-form-item>
        <n-form-item label="显示名" required>
          <n-input v-model:value="form.displayName" placeholder="如 Team Review Model" />
        </n-form-item>
        <n-form-item label="能力" required>
          <n-checkbox-group v-model:value="form.capabilities">
            <n-space>
              <n-checkbox value="chat" label="对话（chat）" />
              <n-checkbox value="structuredOutput" label="结构化输出" />
              <n-checkbox value="toolCalling" label="工具调用" />
              <n-checkbox value="vision" label="视觉（预留）" />
            </n-space>
          </n-checkbox-group>
          <template #feedback>Review / 诊断 / 冲突任务需要「结构化输出」能力</template>
        </n-form-item>
        <n-form-item label="上下文长度">
          <n-input-number
            v-model:value="form.maxContextTokens"
            :min="0"
            :step="1000"
            placeholder="0 = 未知"
          />
          <template #feedback>maxContextTokens，用于上下文预算计算</template>
        </n-form-item>
        <n-form-item label="temperature">
          <n-input-number
            v-model:value="form.temperature"
            :min="0"
            :max="2"
            :step="0.1"
            placeholder="留空使用默认"
          />
        </n-form-item>
        <n-form-item label="启用">
          <n-switch v-model:value="form.enabled" />
        </n-form-item>
      </n-form>
      <template #footer>
        <div class="dialog-footer">
          <n-button @click="form.show = false">取消</n-button>
          <n-button type="primary" :loading="form.saving" :disabled="!valid" @click="save">
            保存
          </n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, reactive, ref } from "vue";
import { useDialog, useMessage, NButton, NTag } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import { aiRemoveModel, aiSaveModel } from "@/api/ai";
import { errMsg } from "@/utils/error";
import type { AiModel, AiProvider, ModelCapability } from "@/types/ai";

const props = defineProps<{ providers: AiProvider[]; models: AiModel[] }>();
const emit = defineEmits<{ refresh: [] }>();

const message = useMessage();
const dialog = useDialog();

const capabilityLabel: Record<ModelCapability, string> = {
  chat: "对话",
  structuredOutput: "结构化输出",
  toolCalling: "工具调用",
  vision: "视觉",
};

const providerFilter = ref<string | null>(null);
const filterOptions = computed(() =>
  props.providers.map((p) => ({ label: p.name, value: p.id })),
);
const filteredModels = computed(() =>
  providerFilter.value ? props.models.filter((m) => m.providerId === providerFilter.value) : props.models,
);

const providerOptions = computed(() =>
  props.providers.map((p) => ({ label: p.name, value: p.id })),
);

const providerName = (id: string) => props.providers.find((p) => p.id === id)?.name ?? id;

const form = reactive({
  show: false,
  saving: false,
  editing: false,
  providerId: "",
  id: "",
  displayName: "",
  capabilities: [] as ModelCapability[],
  maxContextTokens: 0,
  temperature: null as number | null,
  enabled: true,
});

const valid = computed(
  () =>
    form.providerId.length > 0 &&
    form.id.trim().length > 0 &&
    form.displayName.trim().length > 0 &&
    form.capabilities.length > 0,
);

function openCreate() {
  Object.assign(form, {
    show: true,
    saving: false,
    editing: false,
    providerId: props.providers[0]?.id ?? "",
    id: "",
    displayName: "",
    capabilities: ["chat"] as ModelCapability[],
    maxContextTokens: 128000,
    temperature: null,
    enabled: true,
  });
}

function openEdit(m: AiModel) {
  Object.assign(form, {
    show: true,
    saving: false,
    editing: true,
    providerId: m.providerId,
    id: m.id,
    displayName: m.displayName,
    capabilities: [...m.capabilities],
    maxContextTokens: m.maxContextTokens,
    temperature: m.defaults.temperature ?? null,
    enabled: m.enabled,
  });
}

async function save() {
  form.saving = true;
  try {
    await aiSaveModel({
      providerId: form.providerId,
      id: form.id.trim(),
      displayName: form.displayName.trim(),
      capabilities: form.capabilities,
      maxContextTokens: form.maxContextTokens ?? 0,
      defaults: { temperature: form.temperature ?? undefined },
      enabled: form.enabled,
    });
    message.success("已保存");
    form.show = false;
    emit("refresh");
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  } finally {
    form.saving = false;
  }
}

function confirmRemove(m: AiModel) {
  dialog.warning({
    title: "删除模型",
    content: `确定删除「${m.displayName}（${m.id}）」吗？引用它的任务默认值会一并清除。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await aiRemoveModel(m.providerId, m.id);
        message.success("已删除");
        emit("refresh");
      } catch (e) {
        message.error("删除失败: " + errMsg(e));
      }
    },
  });
}

const columns = [
  {
    title: "模型 ID",
    key: "id",
    render: (m: AiModel) => h("span", { class: "mono" }, m.id),
  },
  { title: "显示名", key: "displayName" },
  {
    title: "Provider",
    key: "provider",
    render: (m: AiModel) => providerName(m.providerId),
  },
  {
    title: "能力",
    key: "capabilities",
    render: (m: AiModel) =>
      h(
        "div",
        { class: "cap-cell" },
        m.capabilities.map((c) =>
          h(NTag, { size: "small", bordered: false }, { default: () => capabilityLabel[c] }),
        ),
      ),
  },
  {
    title: "上下文",
    key: "maxContextTokens",
    render: (m: AiModel) =>
      m.maxContextTokens > 0 ? `${m.maxContextTokens.toLocaleString()} tok` : "未知",
  },
  {
    title: "启用",
    key: "enabled",
    width: 70,
    render: (m: AiModel) =>
      m.enabled
        ? h(NTag, { size: "small", type: "success", bordered: false }, { default: () => "是" })
        : h(NTag, { size: "small", type: "error", bordered: false }, { default: () => "否" }),
  },
  {
    title: "操作",
    key: "actions",
    width: 140,
    render: (m: AiModel) =>
      h("div", { class: "row-actions" }, [
        h(NButton, { size: "small", onClick: () => openEdit(m) }, { default: () => "编辑" }),
        h(
          NButton,
          { size: "small", quaternary: true, type: "error", onClick: () => confirmRemove(m) },
          { default: () => "删除" },
        ),
      ]),
  },
];
</script>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.section-toolbar {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: var(--gw-space-2);
}

.provider-filter {
  width: 240px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--gw-space-2);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: 12px;
}

.cap-cell {
  display: flex;
  gap: var(--gw-space-1);
  flex-wrap: wrap;
}

.row-actions {
  display: flex;
  gap: var(--gw-space-1);
}
</style>
