<template>
  <div class="cleaner-tool">
    <n-alert type="info" :bordered="false">
      扫描仅预览，不删除任何内容；删除前需输入 DELETE 确认，且永久删除不进回收站。
      只有「文件」类规则会删除文件，目录规则只删目录。
    </n-alert>

    <!-- 父路径 -->
    <div class="section">
      <div class="section-title">父路径（扫描范围）</div>
      <div class="path-row">
        <n-select
          v-model:value="form.operationPaths"
          multiple
          filterable
          tag
          :options="workspaceOptions"
          :disabled="busy"
          placeholder="输入绝对路径回车添加，或从工作区选择"
        />
        <n-select
          :value="null"
          filterable
          :options="workspaceOptions"
          :disabled="busy"
          placeholder="从工作区快速添加…"
          class="workspace-picker"
          @update:value="addWorkspacePath"
        />
      </div>
    </div>

    <!-- 排除目录 -->
    <div class="section">
      <div class="section-title">排除目录</div>
      <n-select
        v-model:value="form.excludePaths"
        multiple
        filterable
        tag
        :disabled="busy"
        placeholder="排除文件/目录的绝对路径，回车添加；命中的目录不会整体删除"
      />
    </div>

    <!-- 匹配规则 -->
    <div class="section">
      <div class="section-title section-title-row">
        <span>匹配规则</span>
        <div class="rule-actions">
          <n-dropdown trigger="click" :options="presetOptions" @select="addPreset">
            <n-button size="small" quaternary :disabled="busy">常用预设</n-button>
          </n-dropdown>
          <n-button size="small" :disabled="busy" @click="addRule">添加规则</n-button>
        </div>
      </div>
      <div v-for="(r, i) in form.rules" :key="i" class="rule-row">
        <n-input
          v-model:value="r.pattern"
          placeholder="名称，如 node_modules 或 *.tmp"
          :disabled="busy"
          class="rule-pattern"
        />
        <n-select
          v-model:value="r.matchMode"
          :options="matchModeOptions"
          :disabled="busy"
          class="rule-mode"
        />
        <n-select
          v-model:value="r.kind"
          :options="kindOptions"
          :disabled="busy"
          class="rule-kind"
        />
        <n-button quaternary circle size="small" :disabled="busy" @click="form.rules.splice(i, 1)">
          <template #icon><n-icon><TrashOutline /></n-icon></template>
        </n-button>
      </div>
      <div v-if="!form.rules.length" class="empty-hint">
        尚未配置规则，点击「添加规则」或「常用预设」
      </div>
    </div>

    <!-- 操作 -->
    <div class="actions">
      <n-button type="primary" :loading="scanning" :disabled="!canScan" @click="onScan">
        <template #icon><n-icon><SearchOutline /></n-icon></template>
        扫描预览
      </n-button>
      <span v-if="scanning && progress" class="hint">
        已检查 {{ progress.visited }} 项 · 命中 {{ progress.matched }} 项
      </span>
      <span v-else-if="scanResult" class="hint">
        已检查 {{ scanResult.visited }} 项 · 命中 {{ scanResult.matched }} 项 · 待删除
        {{ items.length }} 项
      </span>
    </div>

    <!-- 扫描告警 -->
    <n-alert
      v-if="scanResult && scanResult.warnings.length"
      type="warning"
      :bordered="false"
      title="扫描告警"
    >
      <div class="warnings">
        <div v-for="(w, i) in scanResult.warnings" :key="i" class="mono">{{ w }}</div>
      </div>
    </n-alert>

    <!-- 预览结果 -->
    <template v-if="items.length">
      <n-data-table
        v-model:checked-row-keys="checkedKeys"
        :columns="columns"
        :data="items"
        :row-key="(row: CleanerItemInfo) => row.path"
        virtual-scroll
        max-height="380"
        size="small"
      />
      <div class="actions">
        <n-button
          type="error"
          :loading="executing"
          :disabled="!checkedKeys.length"
          @click="onDelete"
        >
          <template #icon><n-icon><TrashBinOutline /></n-icon></template>
          删除选中项（{{ checkedKeys.length }}）
        </n-button>
        <span class="hint">{{ selectedSizeText }}</span>
      </div>
    </template>
    <n-empty v-else-if="scanResult && !scanning" description="没有命中任何可删除目标" />

    <!-- 删除结果 -->
    <template v-if="executeResult">
      <n-alert :type="executeResult.failed === 0 ? 'success' : 'error'" :bordered="false">
        删除完成：成功 {{ executeResult.deleted }} 项<template v-if="executeResult.failed">
          ，失败 {{ executeResult.failed }} 项</template
        >。本次扫描会话已失效，继续删除请重新扫描。
      </n-alert>
      <div v-if="failedItems.length" class="failed-list">
        <div v-for="item in failedItems" :key="item.path" class="failed-item">
          <div class="mono">{{ item.path }}</div>
          <div class="failed-reason">{{ item.reason }}</div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from "vue";
import {
  NAlert,
  NButton,
  NDataTable,
  NDropdown,
  NEmpty,
  NIcon,
  NInput,
  NSelect,
  NTag,
  useDialog,
  useMessage,
  type DataTableColumns,
  type SelectOption,
} from "naive-ui";
import { SearchOutline, TrashBinOutline, TrashOutline } from "@vicons/ionicons5";
import {
  cleanerComputeSizes,
  cleanerExecute,
  cleanerScan,
  onCleanerScanProgress,
  onCleanerSizeProgress,
  type CleanerItemInfo,
  type CleanerRule,
  type CleanerScanResult,
  type CleanerExecuteResult,
} from "@/api/cleaner";
import { listWorkspaces } from "@/api/workspace";
import { formatSize } from "@/utils/format";
import { errMsg } from "@/utils/error";
import { prompt } from "@/utils/prompt";

const STORAGE_KEY = "gw.toolbox.cleaner.config";

const message = useMessage();
const dialog = useDialog();

// ---------------------------------------------------------------------------
// 配置（localStorage 持久化）
// ---------------------------------------------------------------------------

interface RuleForm {
  pattern: string;
  matchMode: CleanerRule["matchMode"];
  kind: CleanerRule["kind"];
}

const form = reactive<{
  operationPaths: string[];
  excludePaths: string[];
  rules: RuleForm[];
}>({ operationPaths: [], excludePaths: [], rules: [] });

const matchModeOptions: SelectOption[] = [
  { label: "精确名", value: "exact" },
  { label: "通配符", value: "wildcard" },
];
const kindOptions: SelectOption[] = [
  { label: "目录", value: "dir" },
  { label: "文件", value: "file" },
  { label: "文件和目录", value: "any" },
];

const PRESETS: RuleForm[] = [
  { pattern: "node_modules", matchMode: "exact", kind: "dir" },
  { pattern: "target", matchMode: "exact", kind: "dir" },
  { pattern: "bin", matchMode: "exact", kind: "dir" },
  { pattern: "obj", matchMode: "exact", kind: "dir" },
  { pattern: "dist", matchMode: "exact", kind: "dir" },
  { pattern: "build", matchMode: "exact", kind: "dir" },
  { pattern: ".gradle", matchMode: "exact", kind: "dir" },
  { pattern: "__pycache__", matchMode: "exact", kind: "dir" },
  { pattern: "*.tmp", matchMode: "wildcard", kind: "file" },
  { pattern: "*.log", matchMode: "wildcard", kind: "file" },
];
const kindLabel = (kind: RuleForm["kind"]) =>
  kind === "dir" ? "目录" : kind === "file" ? "文件" : "文件和目录";
const presetOptions = PRESETS.map((p, i) => ({
  label: `${p.pattern}（${kindLabel(p.kind)}）`,
  key: i,
}));

function addRule() {
  form.rules.push({ pattern: "", matchMode: "exact", kind: "dir" });
}

function addPreset(key: number) {
  const preset = PRESETS[key];
  if (!preset) return;
  if (form.rules.some((r) => r.pattern === preset.pattern && r.kind === preset.kind)) return;
  form.rules.push({ ...preset });
}

const workspaceOptions = ref<SelectOption[]>([]);

async function loadWorkspaces() {
  try {
    const list = await listWorkspaces();
    workspaceOptions.value = list.map((w) => ({
      label: `${w.name}（${w.path}）`,
      value: w.path,
    }));
  } catch {
    // 工作区列表加载失败不阻塞手动输入
  }
}

function addWorkspacePath(value: string | null) {
  if (value && !form.operationPaths.includes(value)) {
    form.operationPaths.push(value);
  }
}

function loadConfig() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (!saved) return;
    const cfg = JSON.parse(saved) as Partial<typeof form>;
    form.operationPaths = cfg.operationPaths ?? [];
    form.excludePaths = cfg.excludePaths ?? [];
    form.rules = (cfg.rules ?? []).map((r) => ({
      pattern: r.pattern ?? "",
      matchMode: r.matchMode === "wildcard" ? "wildcard" : "exact",
      kind: r.kind ?? "dir",
    }));
  } catch {
    // 配置损坏时从空白开始
  }
}

watch(
  form,
  () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        operationPaths: form.operationPaths,
        excludePaths: form.excludePaths,
        rules: form.rules,
      }),
    );
  },
  { deep: true },
);

// ---------------------------------------------------------------------------
// 扫描 / 大小 / 删除
// ---------------------------------------------------------------------------

const scanning = ref(false);
const executing = ref(false);
const busy = computed(() => scanning.value || executing.value);

const token = ref("");
const scanResult = ref<CleanerScanResult | null>(null);
const items = shallowRef<CleanerItemInfo[]>([]);
const checkedKeys = ref<string[]>([]);
const progress = ref<{ visited: number; matched: number } | null>(null);
/** 已补算的大小（display path → bytes）；文件在扫描结果里预填。 */
const sizeMap = reactive(new Map<string, number>());
const executeResult = ref<CleanerExecuteResult | null>(null);

const canScan = computed(
  () =>
    !busy.value &&
    form.operationPaths.length > 0 &&
    form.rules.some((r) => r.pattern.trim()),
);

let unlisteners: Array<() => void> = [];

onMounted(() => {
  loadConfig();
  void loadWorkspaces();
  void subscribeEvents();
});

onBeforeUnmount(() => {
  unlisteners.forEach((u) => u());
  unlisteners = [];
});

async function subscribeEvents() {
  unlisteners.push(
    await onCleanerScanProgress((p) => {
      if (p.token !== token.value) return;
      progress.value = { visited: p.visited, matched: p.matched };
    }),
    await onCleanerSizeProgress((p) => {
      if (p.token !== token.value || p.done) return;
      sizeMap.set(p.path, p.size);
    }),
  );
}

async function onScan() {
  const rules: CleanerRule[] = form.rules
    .filter((r) => r.pattern.trim())
    .map((r) => ({ pattern: r.pattern.trim(), matchMode: r.matchMode, kind: r.kind }));
  if (!form.operationPaths.length) {
    message.warning("请先配置父路径");
    return;
  }
  if (!rules.length) {
    message.warning("请至少配置一条匹配规则");
    return;
  }

  scanning.value = true;
  scanResult.value = null;
  executeResult.value = null;
  items.value = [];
  checkedKeys.value = [];
  sizeMap.clear();
  progress.value = { visited: 0, matched: 0 };
  try {
    const res = await cleanerScan({
      operationPaths: form.operationPaths,
      excludePaths: form.excludePaths,
      rules,
    });
    token.value = res.token;
    scanResult.value = res;
    items.value = res.items;
    checkedKeys.value = res.items.map((i) => i.path); // 默认全选
    res.items.forEach((i) => {
      if (i.size != null) sizeMap.set(i.path, i.size);
    });
    if (res.items.some((i) => i.isDir)) {
      // 目录大小后台补算，失败不阻塞预览
      cleanerComputeSizes(res.token).catch(() => {});
    }
  } catch (e) {
    message.error("扫描失败：" + errMsg(e));
  } finally {
    scanning.value = false;
  }
}

// ---------------------------------------------------------------------------
// 结果表格
// ---------------------------------------------------------------------------

function rowSize(row: CleanerItemInfo): number {
  return sizeMap.get(row.path) ?? row.size ?? -1;
}

const columns: DataTableColumns<CleanerItemInfo> = [
  { type: "selection" },
  {
    title: "路径",
    key: "path",
    render(row) {
      return h("span", { class: "mono path-cell", title: row.path }, row.path);
    },
  },
  {
    title: "类型",
    key: "isDir",
    width: 76,
    render(row) {
      return h(
        NTag,
        { size: "small", bordered: false, type: row.isDir ? "info" : "default" },
        { default: () => (row.isDir ? "目录" : "文件") },
      );
    },
  },
  {
    title: "大小",
    key: "size",
    width: 110,
    align: "right",
    sorter: (a, b) => rowSize(a) - rowSize(b),
    render(row) {
      const size = sizeMap.get(row.path) ?? row.size;
      if (size != null) return h("span", { class: "mono" }, formatSize(size));
      return h("span", { class: "mono hint" }, "统计中…");
    },
  },
];

const selectedSizeText = computed(() => {
  if (!checkedKeys.value.length) return "";
  let sum = 0;
  let unknown = 0;
  const byPath = new Map(items.value.map((i) => [i.path, i] as const));
  for (const key of checkedKeys.value) {
    const row = byPath.get(key);
    const size = row ? (sizeMap.get(key) ?? row.size) : null;
    if (size == null) unknown += 1;
    else sum += size;
  }
  const total = `已选 ${checkedKeys.value.length} 项 · 已统计 ${formatSize(sum)}`;
  return unknown > 0 ? `${total}（${unknown} 个目录仍在计算）` : `${total}，预计可释放`;
});

const failedItems = computed(
  () => executeResult.value?.items.filter((i) => !i.ok) ?? [],
);

async function onDelete() {
  const paths = [...checkedKeys.value];
  if (!paths.length) return;
  try {
    await prompt(dialog, {
      title: "确认删除",
      content: `将永久删除选中的 ${paths.length} 项（不进回收站，不可恢复）。请输入 DELETE 确认。`,
      placeholder: "DELETE",
      pattern: /^DELETE$/,
      patternError: "请输入大写 DELETE",
      confirmText: "删除",
    });
  } catch {
    return; // 用户取消
  }

  executing.value = true;
  try {
    const res = await cleanerExecute(token.value, paths);
    executeResult.value = res;
    // 会话已失效（后端一次性），清空预览
    items.value = [];
    checkedKeys.value = [];
    token.value = "";
    scanResult.value = null;
    sizeMap.clear();
    if (res.failed === 0) {
      message.success(`已删除 ${res.deleted} 项`);
    } else {
      message.error(`删除完成：成功 ${res.deleted} 项，失败 ${res.failed} 项`);
    }
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
  } finally {
    executing.value = false;
  }
}
</script>

<style scoped>
.cleaner-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}
.section-title {
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
}
.section-title-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.path-row {
  display: flex;
  gap: var(--gw-space-2);
}
.workspace-picker {
  flex: 0 0 220px;
}
.rule-actions {
  display: flex;
  gap: var(--gw-space-2);
}
.rule-row {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
.rule-pattern {
  flex: 1;
}
.rule-mode {
  flex: 0 0 110px;
}
.rule-kind {
  flex: 0 0 130px;
}
.empty-hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}
.actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}
.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}
.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
.path-cell {
  word-break: break-all;
  white-space: normal;
}
.warnings {
  max-height: 140px;
  overflow: auto;
}
.failed-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  max-height: 220px;
  overflow: auto;
}
.failed-item {
  border-left: 2px solid var(--gw-danger, #d03050);
  padding-left: var(--gw-space-2);
}
.failed-reason {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
  white-space: pre-wrap;
}
</style>
