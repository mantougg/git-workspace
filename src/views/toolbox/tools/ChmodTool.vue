<template>
  <div class="chmod-tool">
    <div class="perm-grid">
      <div class="perm-row" v-for="row in ROWS" :key="row.key">
        <span class="perm-label">{{ row.label }}</span>
        <n-checkbox
          v-for="(bit, i) in row.bits"
          :key="i"
          :checked="bitRead(row.key, i)"
          @update:checked="(v: boolean) => bitWrite(row.key, i, v)"
          size="small"
        >
          {{ bit }}
        </n-checkbox>
      </div>
    </div>

    <div class="result-row">
      <span class="result-label">八进制</span>
      <n-input
        :value="octalText"
        size="small"
        class="octal-input mono"
        maxlength="4"
        placeholder="755"
        @update:value="onOctalInput"
      />
      <span class="mono symbolic">{{ symbolic }}</span>
    </div>

    <div class="command-row">
      <span class="mono command">chmod {{ octalText }} <span class="dim">&lt;文件&gt;</span></span>
      <n-button text size="tiny" @click="copy(`chmod ${octalText} `)">复制</n-button>
    </div>

    <div class="presets">
      <span class="hint">常用：</span>
      <n-button
        v-for="p in PRESETS"
        :key="p.value"
        size="tiny"
        secondary
        @click="value = p.value"
      >
        {{ p.text }}
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NCheckbox, NInput, useMessage } from "naive-ui";
import { errMsg } from "@/utils/error";

const message = useMessage();

/** 9 位权限值（0o000–0o777），复选框与八进制输入双向同步的单一数据源。 */
const value = ref(0o755);

type Role = "owner" | "group" | "other";
const ROWS: { key: Role; label: string; bits: string[] }[] = [
  { key: "owner", label: "所有者", bits: ["读 (r)", "写 (w)", "执行 (x)"] },
  { key: "group", label: "同组", bits: ["读 (r)", "写 (w)", "执行 (x)"] },
  { key: "other", label: "其他", bits: ["读 (r)", "写 (w)", "执行 (x)"] },
];

const PRESETS = [
  { value: 0o755, text: "755 可执行" },
  { value: 0o644, text: "644 普通文件" },
  { value: 0o700, text: "700 私有目录" },
  { value: 0o600, text: "600 私有文件" },
  { value: 0o777, text: "777 全开" },
  { value: 0o400, text: "400 只读" },
];

const ROLE_SHIFT: Record<Role, number> = { owner: 6, group: 3, other: 0 };

function bitRead(role: Role, bitIndex: number): boolean {
  const mask = 1 << (ROLE_SHIFT[role] + (2 - bitIndex));
  return (value.value & mask) !== 0;
}

function bitWrite(role: Role, bitIndex: number, on: boolean) {
  const mask = 1 << (ROLE_SHIFT[role] + (2 - bitIndex));
  value.value = on ? value.value | mask : value.value & ~mask;
}

const octalText = computed(() => value.value.toString(8));

/** 手输八进制：只接受合法位，非法输入忽略；满 3 位即生效。 */
function onOctalInput(text: string) {
  const cleaned = text.replace(/[^0-7]/g, "").slice(0, 4);
  if (cleaned === "") {
    value.value = 0;
    return;
  }
  const n = parseInt(cleaned, 8);
  if (n <= 0o7777) value.value = n;
}

const symbolic = computed(() => {
  let out = "";
  for (const shift of [6, 3, 0]) {
    const bits = (value.value >> shift) & 7;
    out += bits & 4 ? "r" : "-";
    out += bits & 2 ? "w" : "-";
    out += bits & 1 ? "x" : "-";
  }
  return out;
});

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
.chmod-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  max-width: 560px;
}

.perm-grid {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.perm-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}

.perm-label {
  width: 48px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.result-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.result-label {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.octal-input {
  width: 72px;
}

.symbolic {
  font-size: var(--gw-text-lg);
}

.command-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.command {
  padding: var(--gw-space-1) var(--gw-space-2);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
}

.dim {
  color: var(--gw-text-dim);
}

.presets {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  flex-wrap: wrap;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
</style>
