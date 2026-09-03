<template>
  <div class="json-format-tool">
    <n-input
      v-model:value="input"
      type="textarea"
      :autosize="{ minRows: 6, maxRows: 12 }"
      placeholder="粘贴 JSON…"
      class="mono-input"
    />

    <div class="tool-actions">
      <n-button-group size="small">
        <n-button :disabled="!input.trim()" @click="format">格式化</n-button>
        <n-button :disabled="!input.trim()" @click="minify">压缩</n-button>
        <n-button :disabled="!input.trim()" @click="sortFormat">按 key 排序</n-button>
        <n-button :disabled="!output" @click="copyOutput">复制结果</n-button>
        <n-button :disabled="!input && !output" @click="clear">清空</n-button>
      </n-button-group>
      <span class="hint">格式化缩进 2 空格；排序为对象 key 递归字典序</span>
    </div>

    <n-alert v-if="error" type="error" :show-icon="true">
      JSON 解析失败：{{ error }}
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

/** 解析并重新序列化；indent=0 即压缩。失败时置错误并清空结果。 */
function transform(indent: number) {
  error.value = "";
  output.value = "";
  try {
    output.value = JSON.stringify(JSON.parse(input.value), null, indent);
  } catch (e) {
    error.value = errMsg(e);
  }
}

function format() {
  transform(2);
}

function minify() {
  transform(0);
}

/** 递归按 key 字典序重排对象（数组保持元素顺序，元素内部同样递归）。 */
function sortKeys(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortKeys);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => [k, sortKeys(v)]),
    );
  }
  return value;
}

function sortFormat() {
  error.value = "";
  output.value = "";
  try {
    output.value = JSON.stringify(sortKeys(JSON.parse(input.value)), null, 2);
  } catch (e) {
    error.value = errMsg(e);
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
.json-format-tool {
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
