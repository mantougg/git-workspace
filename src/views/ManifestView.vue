<template>
  <div class="manifest-view">

    <!-- Export -->
    <div class="section">
      <div class="section-title">
        <n-icon><CloudUploadOutline /></n-icon>
        导出 Manifest
      </div>
      <div class="section-desc">
        将当前工作区导出为 gitworkspace.json（含每个仓库的 remote URL / 默认分支 /
        分组 / 标签），可据此在新机器重建环境。Manifest 只存纯数据，不含任何凭据。
      </div>
      <div class="section-actions">
        <n-button
          type="primary"
          :loading="exporting"
          :disabled="!workspaceStore.currentWorkspace"
          @click="handleExport"
        >
          <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
          导出 Manifest
        </n-button>
        <span v-if="exportSummary" class="summary-text">{{ exportSummary }}</span>
      </div>
    </div>

    <!-- Import / onboarding -->
    <div class="section">
      <div class="section-title">
        <n-icon><CloudUploadOutline /></n-icon>
        导入 Manifest（新成员入职引导）
      </div>
      <div class="section-desc">
        选择 gitworkspace.json → 选择目标目录 → 预览并批量克隆 →
        扫描加入工作区。克隆走任务队列（并发受限、逐仓库子结果、失败可重试）。
      </div>

      <n-steps :current="importStep" class="import-steps">
        <n-step title="选择 Manifest" />
        <n-step title="选择目标目录" />
        <n-step title="预览并克隆" />
        <n-step title="扫描加入工作区" />
      </n-steps>

      <div class="section-actions">
        <n-button :loading="readingManifest" @click="pickManifestFile">
          <template #icon><n-icon><FolderOpenOutline /></n-icon></template>
          选择 Manifest 文件
        </n-button>
        <template v-if="manifest">
          <n-tag type="success">
            {{ manifest.name }} · {{ manifest.repositories.length }} 个仓库
          </n-tag>
          <span class="summary-text">导出于 {{ manifest.exportedAt }}</span>
        </template>
      </div>

      <div v-if="manifest" class="section-actions">
        <n-button :loading="planning" @click="pickTargetRoot">
          <template #icon><n-icon><FolderOpenOutline /></n-icon></template>
          选择目标目录（workspace 根）
        </n-button>
        <span v-if="targetRoot" class="summary-text">{{ targetRoot }}</span>
      </div>

      <!-- Preview -->
      <template v-if="plan">
        <div class="plan-summary">
          <n-tag type="success">将克隆 {{ plan.toClone }} 个</n-tag>
          <n-tag type="info">已存在跳过 {{ plan.skipExisting }} 个</n-tag>
          <n-tag type="warning">无 URL 不可克隆 {{ plan.noUrl }} 个</n-tag>
          <n-button
            type="primary"
            :disabled="plan.toClone === 0 || cloneSubmitted"
            :loading="submitting"
            style="margin-left: auto"
            @click="confirmClone"
          >
            {{ cloneSubmitted ? "已提交克隆任务" : `开始批量克隆（${plan.toClone}）` }}
          </n-button>
        </div>
        <n-data-table
          :columns="planColumns"
          :data="plan.items"
          size="small"
          :max-height="360"
        />
      </template>

      <!-- Post-clone onboarding -->
      <n-alert
        v-if="cloneSubmitted"
        class="scan-alert"
        type="success"
      >
        已提交 {{ submittedCount }} 个克隆任务，进度与失败重试请见下方任务面板
        （Partial Success：部分失败不影响其余仓库）。
        待任务全部完成后，点击下方按钮扫描加入工作区。
        <n-button
          type="primary"
          size="small"
          :loading="scanning"
          @click="scanIntoWorkspace"
        >
          <template #icon><n-icon><SearchOutline /></n-icon></template>
          扫描加入工作区
        </n-button>
      </n-alert>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import {
  CloudUploadOutline,
  FolderOpenOutline,
  SearchOutline,
} from "@vicons/ionicons5";
import { NTag, useDialog, useMessage } from "naive-ui";
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

const workspaceStore = useWorkspaceStore();
const repositoryStore = useRepositoryStore();
const taskStore = useTaskStore();
const message = useMessage();
const dialog = useDialog();

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
  if (cloneSubmitted.value) return 4;
  if (plan.value) return 3;
  if (manifest.value) return 2;
  return 1;
});

const planColumns = [
  { title: "相对路径", key: "path", minWidth: 200 },
  { title: "名称", key: "name", width: 140 },
  {
    title: "分支",
    key: "defaultBranch",
    width: 110,
    render(row: any) {
      if (row.defaultBranch) {
        return h(NTag, { size: "small" }, { default: () => row.defaultBranch });
      }
      return h("span", { class: "text-muted" }, "—");
    },
  },
  {
    title: "分组",
    key: "group",
    width: 110,
    render(row: any) {
      return h("span", null, row.group ?? "—");
    },
  },
  {
    title: "Remote URL",
    key: "remoteUrl",
    minWidth: 260,
    ellipsis: { tooltip: true },
    render(row: any) {
      if (row.remoteUrl) return h("span", null, row.remoteUrl);
      return h("span", { class: "text-muted" }, "无（本地仓库）");
    },
  },
  {
    title: "动作",
    key: "action",
    width: 130,
    align: "center" as const,
    render(row: any) {
      return h(
        NTag,
        { size: "small", type: actionTagType(row.action) },
        { default: () => actionLabel(row.action) },
      );
    },
  },
];

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
  const wsId = workspaceStore.currentWorkspace?.id;
  if (!wsId) return;
  const filePath = await save({
    title: "导出 Workspace Manifest",
    defaultPath: MANIFEST_FILE_NAME,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (typeof filePath !== "string") return;

  exporting.value = true;
  try {
    const m = await exportWorkspaceManifest(
      wsId,
      filePath,
    );
    const noRemote = m.repositories.filter((r) => !r.remoteUrl).length;
    exportSummary.value = `已导出 ${m.repositories.length} 个仓库到 ${filePath}`;
    if (noRemote > 0) {
      exportSummary.value += `（其中 ${noRemote} 个无 remote，导入时不可克隆）`;
    }
    message.success("Manifest 导出成功");
  } catch (e) {
    message.error("导出失败: " + errMsg(e));
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
    message.error("Manifest 读取/校验失败: " + errMsg(e));
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
    message.error("生成克隆预览失败: " + errMsg(e));
  } finally {
    planning.value = false;
  }
}

async function confirmClone() {
  const p = plan.value;
  if (!p || p.toClone === 0) return;

  const confirmed = await new Promise<boolean>((resolve) => {
    dialog.warning({
      title: "确认批量克隆",
      content:
        `将把 ${p.toClone} 个仓库克隆到 ${p.workspaceRoot}（已存在的 ${p.skipExisting} 个跳过、` +
        `无 URL 的 ${p.noUrl} 个不处理）。克隆走系统 git，凭据使用本机 git 配置。`,
      positiveText: "开始克隆",
      negativeText: "取消",
      onPositiveClick: () => resolve(true),
      onNegativeClick: () => resolve(false),
      onClose: () => resolve(false),
    });
  });
  if (!confirmed) return;

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
    message.success(`已提交 ${tasks.length} 个克隆任务`);
  } catch (e) {
    message.error("提交克隆任务失败: " + errMsg(e));
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
    await new Promise<void>((resolve) => {
      dialog.info({
        title: "需要先添加工作区",
        content:
          "目标目录还不是工作区。请先在首页（Dashboard）通过「工作区管理」把该目录添加为工作区，再执行扫描。",
        positiveText: "知道了",
        onPositiveClick: () => resolve(),
        onClose: () => resolve(),
      });
    });
    return;
  }
  scanning.value = true;
  try {
    await repositoryStore.scanRepositories(ws.id);
    message.success(`扫描完成，已发现 ${repositoryStore.totalCount} 个仓库`);
  } catch (e) {
    message.error("扫描失败: " + errMsg(e));
  } finally {
    scanning.value = false;
  }
}

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
});
</script>

<style scoped>
.manifest-view {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
  overflow-y: auto;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.section {
  border: 1px solid var(--gw-border);
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
  color: var(--gw-text-dim);
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
  color: var(--gw-text-dim);
}

.import-steps {
  margin-bottom: 14px;
}

.plan-summary {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-bottom: 10px;
}

.text-muted {
  color: var(--gw-text-dim);
}

.scan-alert {
  margin-top: 12px;
}

.scan-alert :deep(.el-alert__content) {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}
</style>
