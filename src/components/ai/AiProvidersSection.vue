<template>
  <div class="section">
    <div class="section-toolbar">
      <span class="section-hint">Provider 是 API 服务来源（OpenAI 兼容 / Ark / Ollama / 自定义），一个 Provider 可提供多个模型。</span>
      <n-button type="primary" @click="openCreate">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        新增 Provider
      </n-button>
    </div>

    <n-data-table
      :data="providers"
      :columns="columns"
      :row-key="(p: AiProvider) => p.id"
      empty-text="暂无 Provider，点击「新增 Provider」配置第一个 AI 服务来源"
    />

    <!-- 新增/编辑 Provider -->
    <n-modal
      v-model:show="form.show"
      preset="card"
      :title="form.id ? '编辑 Provider' : '新增 Provider'"
      style="width: 520px"
    >
      <n-form label-width="100px">
        <n-form-item label="名称" required>
          <n-input v-model:value="form.name" placeholder="如 Team OpenAI" />
        </n-form-item>
        <n-form-item label="类型" required>
          <n-select v-model:value="form.kind" :options="kindOptions" />
        </n-form-item>
        <n-form-item label="Base URL" required>
          <n-input
            v-model:value="form.baseUrl"
            :placeholder="urlPlaceholder"
            :input-props="{ spellcheck: false }"
          />
          <template #feedback>远程地址须为 https；本机服务（Ollama 等）可用 http://localhost</template>
        </n-form-item>
        <n-form-item label="网络策略">
          <n-select v-model:value="form.networkPolicy" :options="policyOptions" />
          <template #feedback>localOnly 表示本机服务（无需联网与凭证）</template>
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
import { useDialog, useMessage, NButton, NSwitch, NTag } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import { aiRemoveProvider, aiSaveProvider, aiTestProvider } from "@/api/ai";
import { errMsg } from "@/utils/error";
import type { AiProvider, NetworkPolicy, ProviderKind } from "@/types/ai";

defineProps<{ providers: AiProvider[] }>();
const emit = defineEmits<{ refresh: [] }>();

const message = useMessage();
const dialog = useDialog();

const kindOptions = [
  { label: "OpenAI 兼容", value: "openaiCompatible" },
  { label: "Ark（火山方舟）", value: "ark" },
  { label: "Ollama（本机）", value: "ollama" },
  { label: "自定义", value: "custom" },
];
const policyOptions = [
  { label: "在线（onlineOnly）", value: "onlineOnly" },
  { label: "仅本机（localOnly）", value: "localOnly" },
];

const kindLabel: Record<ProviderKind, string> = {
  openaiCompatible: "OpenAI 兼容",
  ark: "Ark",
  ollama: "Ollama",
  custom: "自定义",
};

const urlPlaceholder = computed(() =>
  form.kind === "ollama" ? "http://localhost:11434" : "https://api.openai.com/v1",
);

const form = reactive({
  show: false,
  saving: false,
  id: null as string | null,
  name: "",
  kind: "openaiCompatible" as ProviderKind,
  baseUrl: "",
  networkPolicy: "onlineOnly" as NetworkPolicy,
  enabled: true,
});

const valid = computed(() => form.name.trim().length > 0 && form.baseUrl.trim().length > 0);

function openCreate() {
  Object.assign(form, {
    show: true,
    saving: false,
    id: null,
    name: "",
    kind: "openaiCompatible",
    baseUrl: "",
    networkPolicy: "onlineOnly",
    enabled: true,
  });
}

function openEdit(p: AiProvider) {
  Object.assign(form, {
    show: true,
    saving: false,
    id: p.id,
    name: p.name,
    kind: p.kind,
    baseUrl: p.baseUrl,
    networkPolicy: p.networkPolicy,
    enabled: p.enabled,
  });
}

async function save() {
  form.saving = true;
  try {
    await aiSaveProvider({
      id: form.id,
      name: form.name.trim(),
      kind: form.kind,
      baseUrl: form.baseUrl.trim(),
      enabled: form.enabled,
      networkPolicy: form.networkPolicy,
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

function confirmRemove(p: AiProvider) {
  dialog.warning({
    title: "删除 Provider",
    content: `确定删除「${p.name}」吗？其下模型与相关任务默认值将一并删除，已录入的 API Key 也会从凭证存储中清除。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await aiRemoveProvider(p.id);
        message.success("已删除");
        emit("refresh");
      } catch (e) {
        message.error("删除失败: " + errMsg(e));
      }
    },
  });
}

const testingId = ref<string | null>(null);

/** 测试连接只展示成功/失败原因（§12.2），不回显响应敏感内容。 */
async function test(p: AiProvider) {
  testingId.value = p.id;
  try {
    const result = await aiTestProvider(p.id);
    if (result.success) {
      message.success(`${p.name}：${result.message}（${result.latencyMs}ms）`, {
        duration: 5000,
      });
    } else {
      message.error(`${p.name}：${result.message}`, { duration: 6000 });
    }
  } catch (e) {
    message.error("测试失败: " + errMsg(e));
  } finally {
    testingId.value = null;
  }
}

/** 启用/禁用切换（复用保存链路）。 */
async function toggleEnabled(p: AiProvider, enabled: boolean) {
  try {
    await aiSaveProvider({
      id: p.id,
      name: p.name,
      kind: p.kind,
      baseUrl: p.baseUrl,
      enabled,
      networkPolicy: p.networkPolicy,
    });
    emit("refresh");
  } catch (e) {
    message.error("切换失败: " + errMsg(e));
  }
}

function credentialTag(p: AiProvider) {
  if (!p.hasCredential) {
    return h(NTag, { size: "small", bordered: false }, { default: () => "未配置" });
  }
  return h(
    NTag,
    { size: "small", type: p.sessionOnlyCredential ? "warning" : "success", bordered: false },
    { default: () => (p.sessionOnlyCredential ? "仅本次会话" : "已保存") },
  );
}

const columns = [
  {
    title: "名称",
    key: "name",
    render: (p: AiProvider) =>
      h("div", { class: "name-cell" }, [
        h("span", { class: "name" }, p.name),
        h(NTag, { size: "small", bordered: false }, { default: () => kindLabel[p.kind] }),
      ]),
  },
  {
    title: "Base URL",
    key: "baseUrl",
    render: (p: AiProvider) => h("span", { class: "mono" }, p.baseUrl),
  },
  {
    title: "网络",
    key: "networkPolicy",
    render: (p: AiProvider) =>
      h(
        NTag,
        {
          size: "small",
          type: p.networkPolicy === "localOnly" ? "info" : "default",
          bordered: false,
        },
        { default: () => (p.networkPolicy === "localOnly" ? "仅本机" : "在线") },
      ),
  },
  {
    title: "凭证",
    key: "credential",
    render: credentialTag,
  },
  {
    title: "启用",
    key: "enabled",
    width: 70,
    render: (p: AiProvider) =>
      h(NSwitch, {
        size: "small",
        value: p.enabled,
        onUpdateValue: (v: boolean) => toggleEnabled(p, v),
      }),
  },
  {
    title: "操作",
    key: "actions",
    width: 220,
    render: (p: AiProvider) =>
      h("div", { class: "row-actions" }, [
        h(
          NButton,
          { size: "small", loading: testingId.value === p.id, onClick: () => test(p) },
          { default: () => "测试连接" },
        ),
        h(NButton, { size: "small", onClick: () => openEdit(p) }, { default: () => "编辑" }),
        h(
          NButton,
          { size: "small", quaternary: true, type: "error", onClick: () => confirmRemove(p) },
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
  justify-content: space-between;
  align-items: center;
  gap: var(--gw-space-3);
}

.section-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--gw-space-2);
}

.name-cell {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.name {
  font-weight: 500;
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: 12px;
}

.row-actions {
  display: flex;
  gap: var(--gw-space-1);
}
</style>
