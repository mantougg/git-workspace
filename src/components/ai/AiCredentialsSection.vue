<template>
  <div class="section">
    <n-alert :type="osStoreAvailable ? 'info' : 'warning'" :show-icon="false">
      <template v-if="osStoreAvailable">
        API Key 保存到 OS 凭证存储（Windows Credential Manager / macOS Keychain /
        Linux Secret Service），不写入任何文件。保存后此处<b>不再显示 Key</b>，仅显示状态。
      </template>
      <template v-else>
        当前环境 OS 凭证存储<b>不可用</b>：只能选择「仅本次会话」临时保存（内存保存、
        进程退出即清除，不落盘）。应用不会回退到普通文件存储。
      </template>
    </n-alert>

    <div v-for="p in providers" :key="p.id" class="cred-card">
      <div class="cred-head">
        <span class="cred-name">{{ p.name }}</span>
        <n-tag size="small" :bordered="false">
          {{ apiTypeLabel[p.apiType] }}
        </n-tag>
        <n-tag
          size="small"
          :type="credentialTagType(p)"
          :bordered="false"
        >
          {{ credentialTagText(p) }}
        </n-tag>
        <span class="cred-ref mono" :title="p.credentialRef ?? ''">
          {{ p.credentialRef ?? "—" }}
        </span>
      </div>

      <div class="cred-body">
        <n-input
          v-model:value="keyInputs[p.id]"
          type="password"
          show-password-on="click"
          :placeholder="
            p.hasCredential ? '输入新 Key 以替换（留空则保持不变）' : '输入 API Key'
          "
          class="key-input"
          :input-props="{ spellcheck: false, autocomplete: 'off' }"
          :disabled="p.networkPolicy === 'localOnly'"
        />
        <n-button
          type="primary"
          :loading="saving[p.id] === 'persist'"
          :disabled="!canSave(p, 'persist')"
          @click="saveKey(p, true)"
        >
          {{ p.hasCredential && !p.sessionOnlyCredential ? "替换（保存到系统）" : "保存到系统" }}
        </n-button>
        <n-button
          :loading="saving[p.id] === 'session'"
          :disabled="!canSave(p, 'session')"
          @click="saveKey(p, false)"
        >
          仅本次会话
        </n-button>
        <n-button
          quaternary
          type="error"
          :loading="saving[p.id] === 'delete'"
          :disabled="!p.hasCredential"
          @click="removeKey(p)"
        >
          删除
        </n-button>
      </div>
      <div v-if="p.networkPolicy === 'localOnly'" class="cred-hint">
        localOnly Provider（本机服务）通常无需 API Key。
      </div>
    </div>

    <n-empty
      v-if="providers.length === 0"
      description="先在「Provider」区块添加 Provider，再在此录入 API Key"
    />
  </div>
</template>

<script setup lang="ts">
import { reactive } from "vue";
import { useDialog, useMessage } from "naive-ui";
import { aiClearCredential, aiSetCredential } from "@/api/ai";
import { errMsg } from "@/utils/error";
import type { AiProvider, ApiType } from "@/types/ai";

const props = defineProps<{ providers: AiProvider[]; osStoreAvailable: boolean }>();
const emit = defineEmits<{ refresh: [] }>();

const message = useMessage();
const dialog = useDialog();

const apiTypeLabel: Record<ApiType, string> = {
  openaiChatCompletions: "OpenAI Chat Completions",
  openaiResponses: "OpenAI Responses",
  anthropicMessages: "Anthropic Messages",
};

/** Key 输入为组件本地状态：保存后立即清空，绝不持久化/回显。 */
const keyInputs = reactive<Record<string, string>>({});
/** 每行按钮的 loading 态：'persist' | 'session' | 'delete' | undefined。 */
const saving = reactive<Record<string, "persist" | "session" | "delete" | undefined>>({});

function credentialTagType(p: AiProvider): "success" | "warning" | "default" {
  if (!p.hasCredential) return "default";
  return p.sessionOnlyCredential ? "warning" : "success";
}

function credentialTagText(p: AiProvider): string {
  if (!p.hasCredential) return "未配置";
  return p.sessionOnlyCredential ? "仅本次会话" : "已保存到系统";
}

function canSave(p: AiProvider, mode: "persist" | "session"): boolean {
  const key = (keyInputs[p.id] ?? "").trim();
  if (key.length === 0) return false;
  if (mode === "persist" && !props.osStoreAvailable) return false;
  return true;
}

async function saveKey(p: AiProvider, persist: boolean) {
  const key = (keyInputs[p.id] ?? "").trim();
  if (!key) return;
  saving[p.id] = persist ? "persist" : "session";
  try {
    const status = await aiSetCredential(p.id, key, persist);
    keyInputs[p.id] = "";
    message.success(
      persist
        ? `「${p.name}」API Key 已保存到系统凭证存储`
        : `「${p.name}」API Key 仅保存在本次会话（不落盘）`,
    );
    if (!status.hasCredential) {
      message.warning("凭证写入未生效，请重试");
    }
    emit("refresh");
  } catch (e) {
    message.error(errMsg(e));
  } finally {
    saving[p.id] = undefined;
  }
}

function removeKey(p: AiProvider) {
  dialog.warning({
    title: "删除 API Key",
    content: `确定删除「${p.name}」的 API Key 吗？删除后 AI 请求将无法认证，需要重新录入。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      saving[p.id] = "delete";
      try {
        await aiClearCredential(p.id);
        keyInputs[p.id] = "";
        message.success("已删除");
        emit("refresh");
      } catch (e) {
        message.error("删除失败: " + errMsg(e));
      } finally {
        saving[p.id] = undefined;
      }
    },
  });
}
</script>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.cred-card {
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md, 8px);
  padding: var(--gw-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.cred-head {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.cred-name {
  font-weight: 500;
}

.cred-ref {
  margin-left: auto;
  color: var(--gw-text-dim);
  font-size: 11px;
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cred-body {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.key-input {
  max-width: 360px;
}

.cred-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.mono {
  font-family: var(--gw-font-mono);
}
</style>
