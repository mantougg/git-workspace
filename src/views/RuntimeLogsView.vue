<template>
  <div class="runtime-logs">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 180px"
          @change="selectWorkspace"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-select
          v-model="selectedApp"
          placeholder="选择 Runtime 应用"
          style="width: 180px"
          @change="onAppChange"
        >
          <el-option
            v-for="c in store.configs"
            :key="c.name"
            :label="c.name"
            :value="c.name"
          />
        </el-select>
        <el-select
          v-model="selectedProcessId"
          placeholder="选择进程"
          style="width: 180px"
          @change="onProcessChange"
        >
          <el-option
            v-for="p in appProcesses"
            :key="p.processId"
            :label="processLabel(p)"
            :value="p.processId"
          />
        </el-select>
        <el-button :disabled="!selectedProcessId" :loading="loading" @click="reload">
          <el-icon><RefreshRight /></el-icon>
          刷新
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button
          type="warning"
          plain
          :disabled="!selectedProcessId"
          @click="onClear"
        >
          <el-icon><Delete /></el-icon>
          清空
        </el-button>
        <el-button
          type="primary"
          plain
          :disabled="!selectedProcessId"
          :loading="exporting"
          @click="onExport"
        >
          <el-icon><Download /></el-icon>
          导出
        </el-button>
      </div>
    </div>

    <!-- Controls -->
    <div class="controls">
      <el-button size="small" :type="paused ? 'primary' : 'default'" @click="togglePause">
        {{ paused ? "继续" : "暂停" }}
      </el-button>
      <el-switch
        v-model="autoScroll"
        active-text="自动滚动"
        size="small"
      />
      <el-select v-model="minLevel" size="small" style="width: 130px" placeholder="级别过滤">
        <el-option label="全部级别" value="" />
        <el-option label="TRACE 及以上" value="trace" />
        <el-option label="DEBUG 及以上" value="debug" />
        <el-option label="INFO 及以上" value="info" />
        <el-option label="WARN 及以上" value="warn" />
        <el-option label="仅 ERROR" value="error" />
      </el-select>
      <el-input
        v-model="searchQuery"
        size="small"
        placeholder="搜索日志内容..."
        clearable
        style="width: 220px"
      />
      <span class="line-count">共 {{ visibleLines.length }} 行（{{ levelBad }} 条 ERROR/WARN）</span>
    </div>

    <!-- Log panel -->
    <div class="log-panel" ref="panelRef">
      <div v-if="displayLines.length === 0" class="log-empty">
        <template v-if="!selectedApp">选择应用与进程后查看日志（构建/运行输出自动落盘 .gitworkspace/logs）</template>
        <template v-else>暂无日志</template>
      </div>
      <div
        v-for="line in visibleLines"
        :key="line.key"
        class="log-line"
        :class="lineClass(line)"
      >
        <span class="log-seq mono">{{ lineNumber(line) }}</span>
        <span v-if="line.level" class="log-level" :class="'lv-' + line.level">
          {{ line.level.toUpperCase().padEnd(5) }}
        </span>
        <span v-else class="log-level lv-none">      </span>
        <span class="log-text">{{ line.text }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { Back, RefreshRight, Delete, Download } from "@element-plus/icons-vue";
import { save } from "@tauri-apps/plugin-dialog";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import * as runtimeApi from "@/api/runtime";
import type { LogEntry, LogLevel, LogLine, RuntimeProcessInfo } from "@/types/runtime";
import { errMsg } from "@/utils/error";

interface DisplayLine {
  key: string;
  lineNumber: string;
  level: LogLevel | null;
  text: string;
}

const router = useRouter();
const { workspaceStore, store, selectedWorkspaceId, selectWorkspace } =
  useRuntimeWorkspace();

const selectedApp = ref<string | null>(null);
const selectedProcessId = ref<number | null>(null);
const loading = ref(false);
const exporting = ref(false);
const paused = ref(false);
const autoScroll = ref(true);
const minLevel = ref("");
const searchQuery = ref("");
const fileLines = ref<LogEntry[]>([]);
const panelRef = ref<HTMLDivElement>();

const LEVEL_ORDER: Record<string, number> = {
  trace: 0,
  debug: 1,
  info: 2,
  warn: 3,
  error: 4,
};

/** 渲染预算：最多渲染最近 3000 行（全局约束 §5，高频输出下保流畅）。 */
const DISPLAY_CAP = 3000;

const appProcesses = computed(() =>
  selectedApp.value
    ? store.processes.filter((p) => p.runtimeName === selectedApp.value)
    : [],
);

const liveLines = computed<LogLine[]>(() =>
  selectedApp.value ? store.logBuffers.get(selectedApp.value) ?? [] : [],
);

/** 历史（文件查询）在前，实时缓冲在后；暂停时冻结（不追加 live）。 */
const displayLines = computed<DisplayLine[]>(() => {
  const history: DisplayLine[] = fileLines.value.map((e, i) => ({
    key: `f-${e.lineNumber}-${i}`,
    lineNumber: String(e.lineNumber),
    level: e.level,
    text: e.text,
  }));
  if (paused.value || !selectedApp.value) return history;
  const live: DisplayLine[] = liveLines.value.map((l, i) => ({
    key: `l-${l.seq}-${i}`,
    lineNumber: String(l.seq),
    level: l.level,
    text: l.line,
  }));
  return [...history, ...live];
});

const visibleLines = computed(() => {
  const minOrd = minLevel.value != null && minLevel.value !== ""
    ? LEVEL_ORDER[minLevel.value]
    : null;
  const q = searchQuery.value.trim().toLowerCase();
  const filtered = displayLines.value.filter((l) => {
    if (minOrd != null && l.level != null && (LEVEL_ORDER[l.level] ?? 0) < minOrd) {
      return false;
    }
    if (q && !l.text.toLowerCase().includes(q)) return false;
    return true;
  });
  // 渲染预算（全局约束 §5）：只渲染最近 DISPLAY_CAP 行，更早的折叠。
  if (filtered.length > DISPLAY_CAP) {
    return filtered.slice(filtered.length - DISPLAY_CAP);
  }
  return filtered;
});

const levelBad = computed(
  () => visibleLines.value.filter((l) => l.level === "error" || l.level === "warn").length,
);

function lineNumber(line: DisplayLine): string {
  return line.lineNumber.padStart(6);
}

function lineClass(line: DisplayLine): string {
  if (!line.level) return "lv-none";
  return `lv-${line.level}`;
}

function statusLabel(status: string): string {
  return status;
}

function processLabel(p: RuntimeProcessInfo): string {
  const pid = p.pid != null ? " (pid " + p.pid + ")" : "";
  return "#" + p.processId + " · " + statusLabel(p.status) + pid;
}

// ------------------------------------------------------------------
// 加载
// ------------------------------------------------------------------

async function reload() {
  if (store.workspaceId == null || selectedProcessId.value == null) return;
  loading.value = true;
  try {
    fileLines.value = await runtimeApi.runtimeGetLogs({
      workspaceId: store.workspaceId,
      runtimeName: selectedApp.value!,
      processId: selectedProcessId.value,
      filter: { limit: 2000 },
    });
  } catch (e) {
    ElMessage.error("加载日志失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function onAppChange() {
  selectedProcessId.value = null;
  fileLines.value = [];
  const processes = appProcesses.value;
  if (processes.length > 0) {
    selectedProcessId.value = processes[0].processId;
    await reload();
  }
}

async function onProcessChange() {
  fileLines.value = [];
  await reload();
}

async function onClear() {
  if (store.workspaceId == null || selectedProcessId.value == null) return;
  try {
    await ElMessageBox.confirm("确定清空该进程的日志吗？此操作不可恢复。", "清空日志", {
      confirmButtonText: "清空",
      cancelButtonText: "取消",
      type: "warning",
    });
  } catch {
    return;
  }
  try {
    await runtimeApi.runtimeClearLogs({
      workspaceId: store.workspaceId,
      runtimeName: selectedApp.value!,
      processId: selectedProcessId.value,
    });
    store.logBuffers.delete(selectedApp.value!);
    await reload();
    ElMessage.success("日志已清空");
  } catch (e) {
    ElMessage.error("清空失败：" + errMsg(e));
  }
}

async function onExport() {
  if (store.workspaceId == null || selectedProcessId.value == null) return;
  const defaultPath = `${selectedApp.value}-${selectedProcessId.value}.log`;
  const dest = await save({
    title: "导出日志",
    defaultPath,
    filters: [{ name: "日志", extensions: ["log", "txt"] }],
  });
  if (!dest) return;
  exporting.value = true;
  try {
    const outcome = await runtimeApi.runtimeExportLogs(
      {
        workspaceId: store.workspaceId,
        runtimeName: selectedApp.value!,
        processId: selectedProcessId.value,
        filter: {
          query: searchQuery.value.trim() || null,
          minLevel: (minLevel.value as LogLevel | null) || null,
        },
      },
      dest,
    );
    ElMessage.success(`已导出 ${outcome.lines} 行 → ${outcome.path}`);
  } catch (e) {
    ElMessage.error("导出失败：" + errMsg(e));
  } finally {
    exporting.value = false;
  }
}

function togglePause() {
  paused.value = !paused.value;
}

// ------------------------------------------------------------------
// 自动滚动
// ------------------------------------------------------------------

watch(
  [visibleLines, autoScroll, paused],
  async () => {
    if (autoScroll.value && !paused.value && panelRef.value) {
      await nextTick();
      panelRef.value.scrollTop = panelRef.value.scrollHeight;
    }
  },
  { flush: "post" },
);

function goBack() {
  router.push({ name: "runtime-dashboard" });
}

onMounted(async () => {
  // 从 Dashboard 带参进入（?name=xxx）。
  const name = new URLSearchParams(window.location.search).get("name");
  if (name && store.configs.some((c) => c.name === name)) {
    selectedApp.value = name;
    await onAppChange();
  }
});
</script>

<style scoped>
.runtime-logs {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  gap: 10px;
  overflow: hidden;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
}
.controls {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
.line-count {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.log-panel {
  flex: 1;
  min-height: 0;
  overflow: auto;
  background: #1e1e1e;
  border-radius: 8px;
  padding: 8px 4px;
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
  line-height: 1.55;
}
.log-empty {
  color: #8a8a8a;
  text-align: center;
  padding: 48px 0;
  font-family: inherit;
}
.log-line {
  display: flex;
  gap: 8px;
  padding: 0 8px;
  white-space: pre-wrap;
  word-break: break-all;
}
.log-line:hover {
  background: rgba(255, 255, 255, 0.06);
}
.log-seq {
  color: #6a737d;
  flex-shrink: 0;
  user-select: none;
}
.log-level {
  flex-shrink: 0;
  font-weight: 600;
  width: 52px;
  user-select: none;
}
.log-text {
  color: #d4d4d4;
}
.log-line.lv-error .log-level {
  color: #f56c6c;
}
.log-line.lv-error .log-text {
  color: #ff8f8f;
}
.log-line.lv-warn .log-level {
  color: #e6a23c;
}
.log-line.lv-warn .log-text {
  color: #e6c07b;
}
.log-line.lv-info .log-level {
  color: #409eff;
}
.log-line.lv-debug .log-level {
  color: #9a9a9a;
}
.log-line.lv-trace .log-level {
  color: #6a737d;
}
.log-line.lv-none .log-level {
  color: #6a737d;
}
</style>
