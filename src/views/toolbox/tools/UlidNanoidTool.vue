<template>
  <div class="id-tool">
    <div class="tool-actions">
      <n-radio-group v-model:value="kind" size="small">
        <n-radio-button value="ulid">ULID</n-radio-button>
        <n-radio-button value="nanoid">NanoID</n-radio-button>
      </n-radio-group>
      <n-input-number
        v-if="kind === 'nanoid'"
        v-model:value="nanoLen"
        :min="4"
        :max="64"
        size="small"
        class="num-input"
      />
      <n-input-number
        v-model:value="count"
        :min="1"
        :max="100"
        size="small"
        class="num-input"
      />
      <n-button type="primary" size="small" @click="generate">生成</n-button>
      <n-button size="small" :disabled="results.length === 0" @click="copyAll">
        全部复制
      </n-button>
    </div>
    <div class="hint">
      <template v-if="kind === 'ulid'">ULID：26 位 Crockford Base32，前 10 位为毫秒时间戳（字典序 = 时间序）</template>
      <template v-else>NanoID：URL 安全字母表（A-Za-z0-9_-），长度可调（默认 21，碰撞概率与 UUID v4 相当）</template>
    </div>

    <div v-if="results.length > 0" class="result-list">
      <div v-for="(id, i) in results" :key="i" class="result-row">
        <span class="mono">{{ id }}</span>
        <n-button text size="tiny" @click="copy(id)">复制</n-button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { NButton, NInputNumber, NRadioButton, NRadioGroup, useMessage } from "naive-ui";
import { errMsg } from "@/utils/error";

const message = useMessage();

const kind = ref<"ulid" | "nanoid">("ulid");
const nanoLen = ref(21);
const count = ref(1);
const results = ref<string[]>([]);

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const NANO_ALPHABET =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/** ULID：48 位毫秒时间 + 80 位随机，Crockford Base32 编码 26 字符。 */
function genUlid(): string {
  // 前 10 字符：毫秒时间戳（BigInt 防 32 位截断）。
  let time = BigInt(Date.now());
  let head = "";
  for (let i = 0; i < 10; i++) {
    head = CROCKFORD[Number(time % 32n)] + head;
    time /= 32n;
  }
  // 后 16 字符：80 位随机，按 5bit 一组读取。
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let bits = 0;
  let value = 0;
  let tail = "";
  for (const byte of bytes) {
    value = (value << 8) | byte;
    bits += 8;
    while (bits >= 5) {
      tail += CROCKFORD[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  return head + tail;
}

/** NanoID：64 字符字母表（2 的幂），byte & 63 均匀无偏。 */
function genNanoid(len: number): string {
  const bytes = new Uint8Array(len);
  crypto.getRandomValues(bytes);
  let out = "";
  for (const byte of bytes) out += NANO_ALPHABET[byte & 63];
  return out;
}

function generate() {
  const n = Math.max(1, Math.min(100, count.value ?? 1));
  results.value = Array.from({ length: n }, () =>
    kind.value === "ulid" ? genUlid() : genNanoid(nanoLen.value ?? 21),
  );
}

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch (e) {
    message.error("复制失败：" + errMsg(e));
  }
}

async function copyAll() {
  await copy(results.value.join("\n"));
}
</script>

<style scoped>
.id-tool {
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

.num-input {
  width: 90px;
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

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-md);
}
</style>
