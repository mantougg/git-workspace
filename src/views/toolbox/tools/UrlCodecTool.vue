<template>
  <div class="url-codec-tool">
    <n-input
      v-model:value="input"
      type="textarea"
      :autosize="{ minRows: 6, maxRows: 12 }"
      placeholder="粘贴文本（编码）或 URL 编码串（解码）…"
      class="mono-input"
    />

    <div class="tool-actions">
      <n-button-group size="small">
        <n-button :disabled="!input" @click="encode">编码 →</n-button>
        <n-button :disabled="!input.trim()" @click="decode">← 解码</n-button>
        <n-button :disabled="!output" @click="copyOutput">复制结果</n-button>
        <n-button :disabled="!input && !output" @click="clear">清空</n-button>
      </n-button-group>
      <span class="hint">按 URL 参数规则编码（encodeURIComponent，空格转为 %20）</span>
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

function encode() {
  error.value = "";
  output.value = encodeURIComponent(input.value);
}

function decode() {
  error.value = "";
  output.value = "";
  try {
    output.value = decodeURIComponent(input.value.trim());
  } catch {
    error.value = "解码失败：输入包含不合法的 % 转义序列";
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
.url-codec-tool {
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
