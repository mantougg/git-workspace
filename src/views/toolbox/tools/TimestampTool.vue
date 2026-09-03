<template>
  <div class="timestamp-tool">
    <!-- 当前时间（秒级刷新） -->
    <div class="section">
      <div class="section-title">现在</div>
      <div class="row">
        <span class="label">Unix 秒</span>
        <span class="mono">{{ nowSeconds }}</span>
        <n-button text size="tiny" @click="copy(String(nowSeconds))">复制</n-button>
      </div>
      <div class="row">
        <span class="label">Unix 毫秒</span>
        <span class="mono">{{ nowMs }}</span>
        <n-button text size="tiny" @click="copy(String(nowMs))">复制</n-button>
      </div>
      <div class="row">
        <span class="label">本地时间</span>
        <span class="mono">{{ formatLocal(new Date(nowMs)) }}</span>
      </div>
    </div>

    <!-- 时间戳 → 日期 -->
    <div class="section">
      <div class="section-title">时间戳 → 日期</div>
      <div class="row">
        <n-input-number
          v-model:value="tsInput"
          :show-button="false"
          placeholder="粘贴 Unix 秒或毫秒"
          class="ts-input"
        />
        <span v-if="tsUnit" class="hint">按{{ tsUnit }}解析</span>
      </div>
      <template v-if="tsDate">
        <div class="row">
          <span class="label">本地时间</span>
          <span class="mono">{{ formatLocal(tsDate) }}</span>
        </div>
        <div class="row">
          <span class="label">ISO 8601</span>
          <span class="mono">{{ tsDate.toISOString() }}</span>
        </div>
      </template>
      <n-alert v-else-if="tsInput != null" type="error" :show-icon="true">
        无效时间戳
      </n-alert>
    </div>

    <!-- 日期 → 时间戳 -->
    <div class="section">
      <div class="section-title">日期 → 时间戳</div>
      <div class="row">
        <n-date-picker
          v-model:value="dateInput"
          type="datetime"
          clearable
          placeholder="选择日期时间"
          class="date-picker"
        />
      </div>
      <template v-if="dateInput != null">
        <div class="row">
          <span class="label">Unix 秒</span>
          <span class="mono">{{ Math.floor(dateInput / 1000) }}</span>
          <n-button text size="tiny" @click="copy(String(Math.floor(dateInput / 1000)))">
            复制
          </n-button>
        </div>
        <div class="row">
          <span class="label">Unix 毫秒</span>
          <span class="mono">{{ dateInput }}</span>
          <n-button text size="tiny" @click="copy(String(dateInput))">复制</n-button>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { NAlert, NButton, NDatePicker, NInputNumber, useMessage } from "naive-ui";
import { errMsg } from "@/utils/error";

const message = useMessage();

// ── 当前时间（秒级刷新，卸载清理） ──────────────────────────
const nowMs = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  timer = setInterval(() => (nowMs.value = Date.now()), 1000);
});
onUnmounted(() => {
  if (timer) clearInterval(timer);
});
const nowSeconds = computed(() => Math.floor(nowMs.value / 1000));

// ── 时间戳 → 日期：按位数自动识别秒（< 1e12）/毫秒 ──────────
const tsInput = ref<number | null>(null);
const tsUnit = computed(() => {
  if (tsInput.value == null) return "";
  return Math.abs(tsInput.value) >= 1e12 ? "毫秒" : "秒";
});
const tsDate = computed(() => {
  const v = tsInput.value;
  if (v == null) return null;
  const ms = Math.abs(v) >= 1e12 ? v : v * 1000;
  const d = new Date(ms);
  return Number.isNaN(d.getTime()) ? null : d;
});

// ── 日期 → 时间戳 ─────────────────────────────────────────
const dateInput = ref<number | null>(null);

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

function formatLocal(d: Date): string {
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
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
</script>

<style scoped>
.timestamp-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-4);
  max-width: 560px;
}

.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.section-title {
  font-size: var(--gw-text-sm);
  font-weight: 600;
  color: var(--gw-text-dim);
}

.row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.label {
  width: 64px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-md);
}

.ts-input {
  width: 240px;
}

.date-picker {
  width: 240px;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}
</style>
