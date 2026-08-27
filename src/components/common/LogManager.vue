<template>
  <n-modal
    :show="visible"
    preset="card"
    title="日志管理"
    style="width: 560px"
    :close-on-click-modal="false"
    @update:show="(v: boolean) => { if (!v) visible = false }"
    @after-enter="load"
  >
    <n-spin :show="loading">
      <div class="log-list">
        <div v-if="files.length === 0 && !loading" class="empty">暂无日志文件</div>
        <div v-for="f in files" :key="f.name" class="log-row">
          <span class="log-name">{{ f.name }}</span>
          <span class="log-size">{{ formatSize(f.sizeBytes) }}</span>
        </div>
      </div>
    </n-spin>
    <template #footer>
      <n-button @click="handleOpen" :disabled="loading">
        打开日志目录
      </n-button>
      <n-button @click="handleExport" :disabled="loading">
        导出日志
      </n-button>
      <n-button type="error" @click="handleClear" :disabled="loading">
        清空日志
      </n-button>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { useMessage, useDialog } from "naive-ui";
import {
  clearLogs,
  exportLogs,
  listLogFiles,
  openLogs,
  type LogFileInfo,
} from "@/api/logs";
import { errMsg } from "@/utils/error";

const message = useMessage();
const dialog = useDialog();

const visible = defineModel<boolean>({ required: true });

const files = ref<LogFileInfo[]>([]);
const loading = ref(false);

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

async function load() {
  loading.value = true;
  try {
    files.value = await listLogFiles();
  } catch (e) {
    message.error("加载日志列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function handleOpen() {
  try {
    await openLogs();
  } catch (e) {
    message.error("打开日志目录失败: " + errMsg(e));
  }
}

async function handleExport() {
  try {
    const dir = await open({
      directory: true,
      multiple: false,
      title: "选择导出目录",
    });
    if (typeof dir !== "string") return;
    const dest = await exportLogs(dir);
    message.success(`日志已导出到: ${dest}`);
  } catch (e) {
    message.error("导出日志失败: " + errMsg(e));
  }
}

async function handleClear() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "清空日志",
        content: "确定清空全部日志文件吗？此操作不可恢复。",
        positiveText: "清空",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  try {
    await clearLogs();
    message.success("日志已清空");
    await load();
  } catch (e) {
    message.error("清空日志失败: " + errMsg(e));
  }
}
</script>

<style scoped>
.log-list {
  min-height: 120px;
}
.log-row {
  display: flex;
  justify-content: space-between;
  padding: 8px 4px;
  border-bottom: 1px solid var(--gw-border);
}
.log-name {
  font-family: monospace;
}
.log-size {
  color: var(--gw-text-dim);
  font-size: 12px;
}
.empty {
  color: var(--gw-text-dim);
  text-align: center;
  padding: 24px 0;
}
</style>
