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
            @update:value="onProviderChange"
          />
        </n-form-item>
        <n-form-item v-if="!form.editing" label="接口模型">
          <n-button
            size="small"
            :loading="fetching"
            :disabled="!form.providerId"
            @click="fetchModels"
          >
            <template #icon><n-icon><CloudDownloadOutline /></n-icon></template>
            从接口获取模型列表
          </n-button>
          <template #feedback>
            拉取 Provider 的 /models 列表后勾选添加，避免手输出错；也可以手动输入。
          </template>
        </n-form-item>
        <n-form-item
          v-if="fetchError"
          :show-label="false"
        >
          <n-alert type="error" :show-icon="false" style="width: 100%">
            {{ fetchError }}
          </n-alert>
        </n-form-item>
        <n-form-item
          v-if="fetchedModels.length > 0"
          :label="`勾选添加（${checkedModels.length}/${fetchedModels.length}）`"
        >
          <div class="model-candidates">
            <n-checkbox-group v-model:value="checkedModels">
              <n-space vertical size="small">
                <n-checkbox
                  v-for="m in fetchedModels"
                  :key="m"
                  :value="m"
                  :disabled="addedModelIds.has(m)"
                  class="mono"
                >
                  {{ m }}
                  <n-tag v-if="addedModelIds.has(m)" size="tiny" :bordered="false">已添加</n-tag>
                </n-checkbox>
              </n-space>
            </n-checkbox-group>
          </div>
        </n-form-item>
        <n-form-item label="模型 ID" :required="checkedModels.length === 0">
          <n-input
            v-model:value="form.id"
            placeholder="如 gpt-4o-mini（Provider 侧的模型名）"
            :input-props="{ spellcheck: false }"
            :disabled="form.editing || checkedModels.length > 0"
          />
          <template #feedback>
            {{
              checkedModels.length > 0
                ? `已勾选 ${checkedModels.length} 个接口模型，保存时批量添加`
                : "未勾选接口模型时按此处手动输入添加单个"
            }}
          </template>
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
          <n-button
            type="primary"
            :loading="form.saving"
            :disabled="!valid"
            @click="save"
          >
            {{ saveLabel }}
          </n-button>
        </div>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, reactive, ref } from "vue";
import { useDialog, useMessage, NButton, NTag, NIcon, NAlert } from "naive-ui";
import { AddOutline, CloudDownloadOutline } from "@vicons/ionicons5";
import { aiRemoveModel, aiSaveModel, aiTestProvider } from "@/api/ai";
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

// 从 Provider 接口拉取的候选模型（复选框批量添加；N-XX 用户反馈）。
const fetching = ref(false);
const fetchError = ref<string | null>(null);
const fetchedModels = ref<string[]>([]);
const checkedModels = ref<string[]>([]);

const addedModelIds = computed(() => {
  if (!form.providerId) return new Set<string>();
  return new Set(
    props.models.filter((m) => m.providerId === form.providerId).map((m) => m.id),
  );
});

const saveLabel = computed(() =>
  !form.editing && checkedModels.value.length > 0
    ? `添加 ${checkedModels.value.length} 个模型`
    : "保存",
);

const valid = computed(() => {
  if (form.providerId.length === 0 || form.capabilities.length === 0) return false;
  if (!form.editing && checkedModels.value.length > 0) return true;
  return form.id.trim().length > 0 && form.displayName.trim().length > 0;
});

function onProviderChange() {
  fetchedModels.value = [];
  checkedModels.value = [];
  fetchError.value = null;
}

/** 调 ai_test_provider 的 GET {base}/models 链路，拉取 Provider 侧模型目录。 */
async function fetchModels() {
  if (!form.providerId) return;
  fetching.value = true;
  fetchError.value = null;
  try {
    const result = await aiTestProvider(form.providerId);
    if (!result.success) {
      fetchError.value = result.message;
      return;
    }
    if (result.models.length === 0) {
      fetchError.value = "接口未返回任何模型（该 Provider 可能不支持 /models 列表），请手动输入";
      return;
    }
    fetchedModels.value = result.models;
    checkedModels.value = [];
    message.success(`拉取到 ${result.models.length} 个模型`);
  } catch (e) {
    fetchError.value = errMsg(e);
  } finally {
    fetching.value = false;
  }
}

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
  fetchedModels.value = [];
  checkedModels.value = [];
  fetchError.value = null;
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
    // 批量模式：勾选了接口模型 → 每个模型按当前表单能力/参数批量创建。
    if (!form.editing && checkedModels.value.length > 0) {
      const ids = checkedModels.value;
      for (const id of ids) {
        await aiSaveModel({
          providerId: form.providerId,
          id,
          displayName: id,
          capabilities: form.capabilities,
          maxContextTokens: form.maxContextTokens ?? 0,
          defaults: { temperature: form.temperature ?? undefined },
          enabled: form.enabled,
        });
      }
      message.success(`已添加 ${ids.length} 个模型`);
      form.show = false;
      emit("refresh");
      return;
    }
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

.model-candidates {
  width: 100%;
  max-height: 220px;
  overflow-y: auto;
  padding: var(--gw-space-2);
  border: 1px solid var(--gw-border-subtle, rgba(128, 128, 128, 0.25));
  border-radius: var(--gw-radius-md, 6px);
}

.model-candidates .mono {
  font-family: var(--gw-font-mono);
  font-size: 12px;
}
</style>
