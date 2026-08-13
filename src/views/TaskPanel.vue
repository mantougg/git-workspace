<template>
  <el-drawer
    v-model="visible"
    direction="btt"
    :size="panelHeight + 'px'"
    :with-header="false"
    class="task-drawer"
  >
    <div class="drawer-resize" @mousedown="startResize"></div>
    <div class="drawer-title">
      <span class="drawer-title-text">任务面板</span>
    </div>
    <div class="task-panel-content">
      <div class="task-panel-toolbar">
        <span class="task-count">
          {{ activeCount }} 个进行中 / {{ finishedCount }} 个已完成
        </span>
        <el-button size="small" text @click="handleClear">
          清除已完成
        </el-button>
      </div>

      <el-scrollbar class="task-scroll">
        <div v-if="tasks.length === 0" class="empty-tasks">
          <el-empty description="暂无任务" :image-size="60" />
        </div>
        <div
          v-for="task in tasks"
          :key="task.id"
          class="task-item"
        >
          <div class="task-info">
            <span class="task-type-badge" :class="taskTypeClass(task)">
              {{ taskTypeLabel(task) }}
            </span>
            <span class="task-repo">{{ task.repoName }}</span>
          </div>
          <div class="task-status">
            <el-tag
              v-if="task.status.type === 'queued'"
              type="info"
              size="small"
            >
              排队中
            </el-tag>
            <el-tag
              v-else-if="task.status.type === 'running'"
              type="warning"
              size="small"
            >
              <el-icon class="is-loading"><Loading /></el-icon>
              执行中
            </el-tag>
            <el-tag
              v-else-if="task.status.type === 'success'"
              type="success"
              size="small"
            >
              成功
            </el-tag>
            <el-tag
              v-else-if="task.status.type === 'failed'"
              type="danger"
              size="small"
            >
              失败
            </el-tag>
          </div>
          <div class="task-actions">
            <el-button
              v-if="task.status.type === 'queued'"
              size="small"
              link
              type="danger"
              @click="handleCancel(task.id)"
            >
              取消
            </el-button>
          </div>
          <div
            v-if="task.status.type === 'failed'"
            class="task-error"
          >
            {{ task.status.error }}
          </div>
        </div>
      </el-scrollbar>

      <!-- Git command output (IDE-style console) -->
      <div class="git-console">
        <div class="git-console-header">
          <span class="git-console-title">Git 命令输出</span>
          <el-button
            v-if="gitLogs.length > 0"
            size="small"
            text
            @click="gitLogs = []"
          >
            清空
          </el-button>
        </div>
        <el-scrollbar height="160px">
          <div v-if="gitLogs.length === 0" class="empty-git-log">
            暂无命令输出
          </div>
          <div
            v-for="(log, i) in gitLogs"
            :key="i"
            class="git-log-item"
            :class="{ 'log-failed': !log.success }"
          >
            <div class="git-log-head">
              <span class="git-log-time">{{ log.time }}</span>
              <span class="git-log-repo">{{ log.repoName }}</span>
              <span class="git-log-cmd">{{ log.command }}</span>
              <el-tag
                :type="log.success ? 'success' : 'danger'"
                size="small"
                effect="plain"
              >
                {{ log.success ? "成功" : "失败" }}
              </el-tag>
            </div>
            <pre v-if="log.output" class="git-log-output">{{
              log.output
            }}</pre>
          </div>
        </el-scrollbar>
      </div>
    </div>
  </el-drawer>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { Loading } from "@element-plus/icons-vue";
import { listen } from "@tauri-apps/api/event";
import { useTaskStore } from "@/stores/task";
import { useTaskProgress } from "@/composables/useTaskProgress";
import type { GitCommandResult, Task } from "@/types/task";

const taskStore = useTaskStore();
useTaskProgress();

interface GitLogEntry extends GitCommandResult {
  time: string;
}

const gitLogs = ref<GitLogEntry[]>([]);
let unlistenGit: (() => void) | null = null;

const panelHeight = ref(420);
let resizeStartY = 0;
let resizeStartH = 0;

/** Start dragging the drawer height from its top edge. */
function startResize(e: MouseEvent) {
  e.preventDefault();
  resizeStartY = e.clientY;
  resizeStartH = panelHeight.value;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
}

function onResizeMove(e: MouseEvent) {
  const delta = resizeStartY - e.clientY; // drag up -> taller
  panelHeight.value = Math.max(
    240,
    Math.min(window.innerHeight * 0.85, resizeStartH + delta),
  );
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
}

onMounted(async () => {
  unlistenGit = await listen<GitCommandResult>("git_command_result", (e) => {
    gitLogs.value.unshift({
      ...e.payload,
      time: new Date().toLocaleTimeString(),
    });
    if (gitLogs.value.length > 50) gitLogs.value.pop();
  });
});

onUnmounted(() => {
  if (unlistenGit) {
    unlistenGit();
    unlistenGit = null;
  }
});

const visible = computed({
  get: () => taskStore.panelVisible,
  set: (val) => { taskStore.panelVisible = val; },
});

const tasks = computed(() => taskStore.tasks);
const activeCount = computed(
  () =>
    tasks.value.filter(
      (t) => t.status.type === "queued" || t.status.type === "running",
    ).length,
);
const finishedCount = computed(
  () =>
    tasks.value.filter(
      (t) =>
        t.status.type === "success" ||
        t.status.type === "failed" ||
        t.status.type === "cancelled" ||
        t.status.type === "partialSuccess",
    ).length,
);

function taskTypeLabel(task: Task): string {
  switch (task.taskType.type) {
    case "fetch":
      return "Fetch";
    case "pull":
      return "Pull";
    case "push":
      return "Push";
    case "commit":
      return "Commit";
  }
}

function taskTypeClass(task: Task): string {
  return `task-type-${task.taskType.type}`;
}

async function handleCancel(taskId: string) {
  await taskStore.cancelTask(taskId);
}

async function handleClear() {
  await taskStore.clearFinished();
}
</script>

<style scoped>
.task-drawer {
  /* Compact header/body spacing for the bottom task drawer. */
}

:deep(.el-drawer__body) {
  padding: 0 12px 8px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.drawer-resize {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 8px;
  cursor: ns-resize;
  z-index: 10;
}

.drawer-resize:hover {
  background: #409eff;
}

.drawer-title {
  display: flex;
  align-items: center;
  padding: 6px 0;
  border-bottom: 1px solid #ebeef5;
  flex-shrink: 0;
}

.drawer-title-text {
  font-size: 13px;
  font-weight: 600;
  color: #303133;
}

.task-panel-content {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding-top: 8px;
}

.task-scroll {
  flex: 1;
  min-height: 0;
}

.task-panel-toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 4px 0 8px;
  border-bottom: 1px solid #ebeef5;
}

.task-count {
  font-size: 13px;
  color: #606266;
}

.task-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-bottom: 1px solid #f0f0f0;
  flex-wrap: wrap;
}

.task-info {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
}

.task-type-badge {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 3px;
  font-weight: 600;
}

.task-type-fetch {
  background: #e6f7ff;
  color: #1890ff;
}

.task-type-pull {
  background: #f6ffed;
  color: #52c41a;
}

.task-type-push {
  background: #fff7e6;
  color: #fa8c16;
}

.task-type-commit {
  background: #fff0f6;
  color: #eb2f96;
}

.task-repo {
  font-size: 13px;
  font-weight: 500;
}

.task-error {
  width: 100%;
  font-size: 12px;
  color: #f56c6c;
  padding: 2px 0;
}

.empty-tasks {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 200px;
}

.git-console {
  margin-top: 8px;
  border-top: 1px solid #ebeef5;
  padding-top: 6px;
}

.git-console-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 4px;
}

.git-console-title {
  font-size: 13px;
  font-weight: 600;
  color: #303133;
}

.empty-git-log {
  font-size: 12px;
  color: #c0c4cc;
  text-align: center;
  padding: 12px 0;
}

.git-log-item {
  padding: 6px 8px;
  border-bottom: 1px solid #f5f5f5;
}

.git-log-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.git-log-time {
  font-size: 11px;
  color: #909399;
  flex-shrink: 0;
}

.git-log-repo {
  font-size: 12px;
  font-weight: 600;
  flex-shrink: 0;
}

.git-log-cmd {
  font-family: Consolas, monospace;
  font-size: 12px;
  color: #409eff;
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.git-log-output {
  font-family: Consolas, monospace;
  font-size: 11px;
  color: #606266;
  background: #f5f7fa;
  padding: 4px 8px;
  border-radius: 3px;
  margin: 4px 0 0;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 80px;
  overflow-y: auto;
}

.git-log-item.log-failed .git-log-cmd {
  color: #f56c6c;
}

.git-log-item.log-failed .git-log-output {
  color: #f56c6c;
  background: #fef0f0;
}
</style>
