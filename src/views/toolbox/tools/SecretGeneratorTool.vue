<template>
  <div class="secret-tool">
    <div class="tool-actions">
      <n-radio-group v-model:value="bits" size="small" @update:value="generate">
        <n-radio-button :value="128">128-bit</n-radio-button>
        <n-radio-button :value="192">192-bit</n-radio-button>
        <n-radio-button :value="256">256-bit</n-radio-button>
      </n-radio-group>
      <n-button type="primary" size="small" :loading="loading" @click="generate">
        重新生成
      </n-button>
    </div>
    <div class="hint">
      加密安全随机数（OS CSPRNG），三种编码同源。可用作「LAN 加密聊天」的 Shared
      Secret——请通过安全渠道（面对面 / IM / 扫码）分发，不要明文发到聊天群里。
    </div>

    <div v-if="secret" class="result-list">
      <div v-for="row in rows" :key="row.label" class="result-row">
        <span class="row-label">{{ row.label }}</span>
        <span class="mono row-value">{{ row.value }}</span>
        <n-button text size="tiny" @click="copy(row.value)">复制</n-button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { NButton, NRadioButton, NRadioGroup, useMessage } from "naive-ui";
import { errMsg } from "@/utils/error";
import { toolboxGenerateSecret, type GeneratedSecret } from "@/api/toolbox";

const message = useMessage();

const bits = ref(256);
const secret = ref<GeneratedSecret | null>(null);
const loading = ref(false);

const rows = computed(() => {
  if (!secret.value) return [];
  return [
    { label: "Hex", value: secret.value.hex },
    { label: "Base64", value: secret.value.base64 },
    { label: "Base64URL", value: secret.value.base64Url },
  ];
});

async function generate() {
  loading.value = true;
  try {
    secret.value = await toolboxGenerateSecret(bits.value);
  } catch (e) {
    message.error("生成失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch (e) {
    message.error("复制失败：" + errMsg(e));
  }
}

onMounted(generate);
</script>

<style scoped>
.secret-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  max-width: 640px;
}

.tool-actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  flex-wrap: wrap;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.result-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.result-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: var(--gw-space-1) var(--gw-space-2);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
}

.row-label {
  flex-shrink: 0;
  width: 80px;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.row-value {
  flex: 1;
  word-break: break-all;
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-md);
}
</style>
