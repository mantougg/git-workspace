<template>
  <el-dialog
    v-model="visible"
    title="日志管理"
    width="560px"
    :close-on-click-modal="false"
    @open="load"
  >
    <div class="log-list" v-loading="loading">
      <div v-if="files.length === 0 && !loading" class="empty">暂无日志文件</div>
      <div v-for="f in files" :key="f.name" class="log-row">
        <span class="log-name">{{ f.name }}</span>
        <span class="log-size">{{ formatSize(f.sizeBytes) }}</span>
      </div>
    </div>
    <template #footer>
      <el-button @click="handleOpen" :disabled="loading">
        打开日志目录
      </el-button>
      <el-button @click="handleExport" :disabled="loading">
        导出日志
      </el-button>
      <el-button type="danger" @click="handleClear" :disabled="loading">
        清空日志
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { open } from "@tauri-apps/plugin-dialog";
import {
  clearLogs,
  exportLogs,
  listLogFiles,
  openLogs,
  type LogFileInfo,
} from "@/api/logs";
import { errMsg } from "@/utils/error";

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
    ElMessage.error("加载日志列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function handleOpen() {
  try {
    await openLogs();
  } catch (e) {
    ElMessage.error("打开日志目录失败: " + errMsg(e));
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
    ElMessage.success(`日志已导出到: ${dest}`);
  } catch (e) {
    ElMessage.error("导出日志失败: " + errMsg(e));
  }
}

async function handleClear() {
  try {
    await ElMessageBox.confirm("确定清空全部日志文件吗？此操作不可恢复。", "清空日志", {
      type: "warning",
      confirmButtonText: "清空",
      cancelButtonText: "取消",
    });
  } catch {
    return;
  }
  try {
    await clearLogs();
    ElMessage.success("日志已清空");
    await load();
  } catch (e) {
    ElMessage.error("清空日志失败: " + errMsg(e));
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
  border-bottom: 1px solid #f0f0f0;
}
.log-name {
  font-family: monospace;
}
.log-size {
  color: #909399;
  font-size: 12px;
}
.empty {
  color: #909399;
  text-align: center;
  padding: 24px 0;
}
</style>
