<template>
  <div class="manifest-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          @change="onWorkspaceChange"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
      </div>
    </div>

    <!-- Export -->
    <div class="section">
      <div class="section-title">
        <el-icon><Download /></el-icon>
        导出 Manifest
      </div>
      <div class="section-desc">
        将当前工作区导出为 gitworkspace.json（含每个仓库的 remote URL / 默认分支 /
        分组 / 标签），可据此在新机器重建环境。Manifest 只存纯数据，不含任何凭据。
      </div>
      <div class="section-actions">
        <el-button
          type="primary"
          :loading="exporting"
          :disabled="!selectedWorkspaceId"
          @click="handleExport"
        >
          <el-icon><Download /></el-icon>
          导出 Manifest
        </el-button>
        <span v-if="exportSummary" class="summary-text">{{ exportSummary }}</span>
      </div>
    </div>

    <!-- Import / onboarding -->
    <div class="section">
      <div class="section-title">
        <el-icon><Upload /></el-icon>
        导入 Manifest（新成员入职引导）
      </div>
      <div class="section-desc">
        选择 gitworkspace.json → 选择目标目录 → 预览并批量克隆 →
        扫描加入工作区。克隆走任务队列（并发受限、逐仓库子结果、失败可重试）。
      </div>

      <el-steps :active="importStep" align-center class="import-steps">
        <el-step title="选择 Manifest" />
        <el-step title="选择目标目录" />
        <el-step title="预览并克隆" />
        <el-step title="扫描加入工作区" />
      </el-steps>

      <div class="section-actions">
        <el-button :loading="readingManifest" @click="pickManifestFile">
          <el-icon><FolderOpened /></el-icon>
          选择 Manifest 文件
        </el-button>
        <template v-if="manifest">
          <el-tag type="success" effect="plain">
            {{ manifest.name }} · {{ manifest.repositories.length }} 个仓库
          </el-tag>
          <span class="summary-text">导出于 {{ manifest.exportedAt }}</span>
        </template>
      </div>

      <div v-if="manifest" class="section-actions">
        <el-button :loading="planning" @click="pickTargetRoot">
          <el-icon><FolderOpened /></el-icon>
          选择目标目录（workspace 根）
        </el-button>
        <span v-if="targetRoot" class="summary-text">{{ targetRoot }}</span>
      </div>

      <!-- Preview -->
      <template v-if="plan">
        <div class="plan-summary">
          <el-tag type="success">将克隆 {{ plan.toClone }} 个</el-tag>
          <el-tag type="info">已存在跳过 {{ plan.skipExisting }} 个</el-tag>
          <el-tag type="warning">无 URL 不可克隆 {{ plan.noUrl }} 个</el-tag>
          <el-button
            type="primary"
            :disabled="plan.toClone === 0 || cloneSubmitted"
            :loading="submitting"
            style="margin-left: auto"
            @click="confirmClone"
          >
            {{ cloneSubmitted ? "已提交克隆任务" : `开始批量克隆（${plan.toClone}）` }}
          </el-button>
        </div>
        <el-table :data="plan.items" size="small" height="360">
          <el-table-column prop="path" label="相对路径" min-width="200" />
          <el-table-column prop="name" label="名称" width="140" />
          <el-table-column label="分支" width="110">
            <template #default="{ row }">
              <el-tag v-if="row.defaultBranch" size="small" effect="plain">
                {{ row.defaultBranch }}
              </el-tag>
              <span v-else class="text-muted">—</span>
            </template>
          </el-table-column>
          <el-table-column label="分组" width="110">
            <template #default="{ row }">
              <span>{{ row.group ?? "—" }}</span>
            </template>
          </el-table-column>
          <el-table-column label="Remote URL" min-width="260" show-overflow-tooltip>
            <template #default="{ row }">
              <span v-if="row.remoteUrl">{{ row.remoteUrl }}</span>
              <span v-else class="text-muted">无（本地仓库）</span>
            </template>
          </el-table-column>
          <el-table-column label="动作" width="130" align="center">
            <template #default="{ row }">
              <el-tag size="small" :type="actionTagType(row.action)">
                {{ actionLabel(row.action) }}
              </el-tag>
            </template>
          </el-table-column>
        </el-table>
      </template>

      <!-- Post-clone onboarding -->
      <el-alert
        v-if="cloneSubmitted"
        class="scan-alert"
        type="success"
        :closable="false"
        show-icon
      >
        <template #title>
          已提交 {{ submittedCount }} 个克隆任务，进度与失败重试请见下方任务面板
          （Partial Success：部分失败不影响其余仓库）。
          待任务全部完成后，点击下方按钮扫描加入工作区。
        </template>
        <el-button
          type="primary"
          size="small"
          :loading="scanning"
          @click="scanIntoWorkspace"
        >
          <el-icon><Search /></el-icon>
          扫描加入工作区
        </el-button>
      </el-alert>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  Back,
  Download,
  FolderOpened,
  Search,
  Upload,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskStore } from "@/stores/task";
import {
  exportWorkspaceManifest,
  planManifestClone,
  readManifestFile,
} from "@/api/manifest";
import { submitTasks } from "@/api/task";
import type { TaskRequest } from "@/types/task";
import type {
  CloneAction,
  ClonePlan,
  WorkspaceManifest,
} from "@/types/manifest";
import { errMsg } from "@/utils/error";

const MANIFEST_FILE_NAME = "gitworkspace.json";

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const repositoryStore = useRepositoryStore();
const taskStore = useTaskStore();

const selectedWorkspaceId = ref<number | null>(null);
const exporting = ref(false);
const exportSummary = ref("");

const readingManifest = ref(false);
const manifest = ref<WorkspaceManifest | null>(null);
const targetRoot = ref("");
const planning = ref(false);
const plan = ref<ClonePlan | null>(null);
const submitting = ref(false);
const cloneSubmitted = ref(false);
const submittedCount = ref(0);
const scanning = ref(false);

const importStep = computed(() => {
  if (cloneSubmitted.value) return 3;
  if (plan.value) return 2;
  if (manifest.value) return 1;
  return 0;
});

function actionLabel(action: CloneAction): string {
  switch (action) {
    case "clone":
      return "将克隆";
    case "skipExisting":
      return "已存在跳过";
    case "noUrl":
      return "无 URL 不可克隆";
  }
}

function actionTagType(action: CloneAction): "success" | "info" | "warning" {
  switch (action) {
    case "clone":
      return "success";
    case "skipExisting":
      return "info";
    case "noUrl":
      return "warning";
  }
}

async function handleExport() {
  if (!selectedWorkspaceId.value) return;
  const filePath = await save({
    title: "导出 Workspace Manifest",
    defaultPath: MANIFEST_FILE_NAME,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof filePath !== "string") return;

  exporting.value = true;
  try {
    const m = await exportWorkspaceManifest(
      selectedWorkspaceId.value,
      filePath,
    );
    const noRemote = m.repositories.filter((r) => !r.remoteUrl).length;
    exportSummary.value = `已导出 ${m.repositories.length} 个仓库到 ${filePath}`;
    if (noRemote > 0) {
      exportSummary.value += `（其中 ${noRemote} 个无 remote，导入时不可克隆）`;
    }
    ElMessage.success("Manifest 导出成功");
  } catch (e) {
    ElMessage.error("导出失败: " + errMsg(e));
  } finally {
    exporting.value = false;
  }
}

async function pickManifestFile() {
  const filePath = await open({
    title: "选择 Workspace Manifest",
    multiple: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof filePath !== "string") return;

  readingManifest.value = true;
  try {
    manifest.value = await readManifestFile(filePath);
    // Reset downstream state when a new file is picked.
    targetRoot.value = "";
    plan.value = null;
    cloneSubmitted.value = false;
    submittedCount.value = 0;
  } catch (e) {
    manifest.value = null;
    ElMessage.error("Manifest 读取/校验失败: " + errMsg(e));
  } finally {
    readingManifest.value = false;
  }
}

async function pickTargetRoot() {
  const dir = await open({
    title: "选择目标目录（克隆存放的 workspace 根）",
    directory: true,
    multiple: false,
  });
  if (typeof dir !== "string" || !manifest.value) return;

  planning.value = true;
  try {
    plan.value = await planManifestClone(manifest.value, dir);
    targetRoot.value = dir;
    cloneSubmitted.value = false;
    submittedCount.value = 0;
  } catch (e) {
    plan.value = null;
    ElMessage.error("生成克隆预览失败: " + errMsg(e));
  } finally {
    planning.value = false;
  }
}

async function confirmClone() {
  const p = plan.value;
  if (!p || p.toClone === 0) return;

  try {
    await ElMessageBox.confirm(
      `将把 ${p.toClone} 个仓库克隆到 ${p.workspaceRoot}（已存在的 ${p.skipExisting} 个跳过、` +
        `无 URL 的 ${p.noUrl} 个不处理）。克隆走系统 git，凭据使用本机 git 配置。`,
      "确认批量克隆",
      {
        type: "warning",
        confirmButtonText: "开始克隆",
        cancelButtonText: "取消",
      },
    );
  } catch {
    return;
  }

  const tasks: TaskRequest[] = p.items
    .filter((i) => i.action === "clone" && i.remoteUrl)
    .map((i) => ({
      taskType: {
        type: "clone",
        url: i.remoteUrl as string,
        branch: i.defaultBranch ?? null,
      },
      repoPath: i.destPath,
      repoName: i.name,
    }));

  submitting.value = true;
  try {
    await submitTasks(tasks);
    cloneSubmitted.value = true;
    submittedCount.value = tasks.length;
    await taskStore.loadActiveTasks();
    taskStore.showPanel();
    ElMessage.success(`已提交 ${tasks.length} 个克隆任务`);
  } catch (e) {
    ElMessage.error("提交克隆任务失败: " + errMsg(e));
  } finally {
    submitting.value = false;
  }
}

/** Normalize for path comparison across dialog / DB path forms. */
function normPath(p: string): string {
  return p.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();
}

async function scanIntoWorkspace() {
  const root = normPath(targetRoot.value);
  const ws = workspaceStore.workspaces.find((w) => normPath(w.path) === root);
  if (!ws) {
    await ElMessageBox.alert(
      "目标目录还不是工作区。请先在首页（Dashboard）通过「工作区管理」把该目录添加为工作区，再执行扫描。",
      "需要先添加工作区",
      { confirmButtonText: "知道了" },
    );
    return;
  }
  scanning.value = true;
  try {
    await repositoryStore.scanRepositories(ws.id);
    ElMessage.success(`扫描完成，已发现 ${repositoryStore.totalCount} 个仓库`);
  } catch (e) {
    ElMessage.error("扫描失败: " + errMsg(e));
  } finally {
    scanning.value = false;
  }
}

function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  const ws = workspaceStore.workspaces.find((w) => w.id === id);
  if (ws) workspaceStore.selectWorkspace(ws);
}

function goBack() {
  router.push({ name: "dashboard" });
}

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
  }
});
</script>

<style scoped>
.manifest-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  gap: 12px;
  overflow-y: auto;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: 8px;
  align-items: center;
}

.section {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px 14px;
}

.section-title {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 15px;
  font-weight: 600;
  margin-bottom: 8px;
}

.section-desc {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 12px;
}

.section-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
}

.summary-text {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.import-steps {
  margin-bottom: 14px;
}

.plan-summary {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}

.text-muted {
  color: var(--el-text-color-secondary);
}

.scan-alert {
  margin-top: 12px;
}

.scan-alert :deep(.el-alert__content) {
  display: flex;
  align-items: center;
  gap: 12px;
}
</style>
