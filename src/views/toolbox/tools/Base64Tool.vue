<template>
  <div class="base64-tool">
    <n-input
      v-model:value="input"
      type="textarea"
      :autosize="{ minRows: 6, maxRows: 12 }"
      placeholder="粘贴文本（编码）或 Base64 串（解码）…"
      class="mono-input"
    />

    <div class="tool-actions">
      <n-button-group size="small">
        <n-button :disabled="!input" @click="encode">编码 →</n-button>
        <n-button :disabled="!input.trim()" @click="decode">← 解码</n-button>
        <n-button :disabled="!output" @click="copyOutput">复制结果</n-button>
        <n-button :disabled="!input && !output" @click="clear">清空</n-button>
      </n-button-group>
      <span class="hint">支持 UTF-8 中文；解码输入会忽略首尾空白</span>
    </div>

    <n-alert v-if="error" type="error" :show-icon="true">
      {{ error }}
    </n-alert>

    <pre v-if="output" class="result mono">{{ output }}</pre>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { NAlert, NButton, NButtonGroup, NInput, useMessage } from "naive-ui";
import { errMsg } from "@/utils/error";

const message = useMessage();

const input = ref("");
const output = ref("");
const error = ref("");

/**
 * UTF-8 安全编解码：btoa/atob 只认 Latin-1，中文需先经
 * TextEncoder/TextDecoder 转字节。分块 String.fromCharCode 避免
 * 大输入时扩展运算撑爆调用栈。
 */
function encodeUtf8Base64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

function decodeUtf8Base64(b64: string): string {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new TextDecoder().decode(bytes);
}

function encode() {
  error.value = "";
  output.value = "";
  try {
    output.value = encodeUtf8Base64(input.value);
  } catch (e) {
    error.value = "编码失败：" + errMsg(e);
  }
}

function decode() {
  error.value = "";
  output.value = "";
  try {
    output.value = decodeUtf8Base64(input.value.trim());
  } catch {
    error.value = "解码失败：输入不是合法的 Base64（或解码结果不是 UTF-8 文本）";
  }
}

async function copyOutput() {
  try {
    await navigator.clipboard.writeText(output.value);
    message.success("已复制到剪贴板");
  } catch (e) {
    message.error("复制失败：" + errMsg(e));
  }
}

function clear() {
  input.value = "";
  output.value = "";
  error.value = "";
}
</script>

<style scoped>
.base64-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.mono-input :deep(textarea) {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}

.tool-actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  flex-wrap: wrap;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.result {
  margin: 0;
  padding: var(--gw-space-3);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
  max-height: 480px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
</style>
