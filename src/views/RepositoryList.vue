<template>
  <div class="repository-list">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
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
        <el-button @click="showAddWorkspace = true">
          <el-icon><Plus /></el-icon>
          添加工作区
        </el-button>
        <el-button
          type="primary"
          :loading="repoStore.scanning"
          :disabled="!selectedWorkspaceId"
          @click="handleScan"
        >
          <el-icon><Refresh /></el-icon>
          扫描仓库
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          @click="toggleWatcher"
        >
          <el-icon><Monitor /></el-icon>
          {{ watcherActive ? "停止监听" : "启动监听" }}
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button
          v-if="taskStore.tasks.length > 0"
          @click="taskStore.togglePanel()"
        >
          <el-icon><Bell /></el-icon>
          任务 ({{ taskStore.tasks.length }})
        </el-button>
        <el-button @click="showLogManager = true">
          <el-icon><FolderOpened /></el-icon>
          日志
        </el-button>
        <el-input
          v-model="searchQuery"
          placeholder="搜索文件或仓库..."
          style="width: 240px"
          clearable
          :prefix-icon="Search"
        />
      </div>
    </div>

    <div class="main-body">
      <div class="tree-pane">
        <!-- Stats summary -->
        <div class="stats-bar">
          <span>共 {{ repoStore.totalCount }} 个仓库</span>
          <span class="separator">|</span>
          <span>{{ dirtyRepoCount }} 个有变更</span>
          <span class="separator">|</span>
          <span>{{ totalChangedFiles }} 个文件变更</span>
          <span v-if="selectedFileCount > 0" class="separator">|</span>
          <span v-if="selectedFileCount > 0" class="selected-info">
            已勾选 {{ selectedRepoCount }} 个仓库 / {{ selectedFileCount }} 个文件
          </span>
          <span class="tree-controls">
            <el-button size="small" @click="expandAll">
              <el-icon><Expand /></el-icon>
              展开全部
            </el-button>
            <el-button size="small" @click="collapseAll">
              <el-icon><Fold /></el-icon>
              收起全部
            </el-button>
          </span>
        </div>

        <!-- Scan progress bar -->
        <div v-if="scanProgress" class="scan-progress">
          <el-progress
            :percentage="scanPercentage"
            :status="scanPercentage === 100 ? 'success' : ''"
            :stroke-width="16"
            :text-inside="true"
            :format="() => `扫描状态 ${scanProgress?.current ?? 0}/${scanProgress?.total ?? 0}`"
          />
        </div>

        <!-- Change tree -->
        <div class="tree-container" v-loading="changesLoading">
          <ChangeTree
            ref="changeTreeRef"
            :changes="changes"
            @selection-change="onTreeSelection"
            @file-dblclick="onFileDblClick"
          />
          <div
            v-if="!changesLoading && selectedWorkspaceId && changes.length === 0"
            class="empty-state"
          >
            <el-empty description="未发现任何 Git 仓库">
              <el-button type="primary" @click="handleScan">重新扫描</el-button>
            </el-empty>
          </div>
          <div
            v-else-if="!selectedWorkspaceId"
            class="empty-state"
          >
            <el-empty description="请先添加工作区目录">
              <el-button type="primary" @click="showAddWorkspace = true">
                添加工作区
              </el-button>
            </el-empty>
          </div>
        </div>
      </div>

      <!-- Right: change content of double-clicked file -->
      <div class="resize-handle" @mousedown="startResize"></div>
      <div
        v-if="selectedDiff"
        ref="diffPaneEl"
        class="diff-pane"
        :style="{ width: diffWidth ? diffWidth + 'px' : '46%' }"
        v-loading="diffLoading"
      >
        <div class="diff-pane-header">
          <div class="diff-pane-title">
            <span class="diff-repo">{{ repoNameOf(selectedDiff.repoPath) }}</span>
            <span class="diff-file">{{ selectedDiff.relPath }}</span>
            <el-tag size="small" effect="plain">
              {{ statusText(selectedDiff.file.status) }}
            </el-tag>
          </div>
          <el-button
            size="small"
            text
            :icon="Close"
            @click="selectedDiff = null"
          />
        </div>
        <div class="diff-pane-body">
          <UnifiedDiff v-if="selectedDiff" :file="selectedDiff.file" />
        </div>
      </div>
    </div>

    <!-- Bottom: batch operations panel (always visible, buttons disable instead of hiding) -->
    <div class="commit-panel">
      <div class="commit-panel-header">
        <el-button
          size="small"
          text
          :icon="commitPanelOpen ? ArrowDown : ArrowUp"
          @click="commitPanelOpen = !commitPanelOpen"
        >
          {{ commitPanelOpen ? "收起" : "展开" }}批量操作
        </el-button>
        <span v-if="commitPanelOpen" class="commit-panel-hint">
          {{ selectedFileCount > 0
            ? `已勾选 ${selectedFileCount} 个文件（${selectedRepoCount} 个仓库）`
            : "在左侧勾选变更文件后即可操作" }}
        </span>
      </div>
      <div v-if="commitPanelOpen" class="commit-panel-body">
        <div class="ops-row">
          <el-button-group>
            <el-button
              size="small"
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleAdd"
            >
              <el-icon><CirclePlus /></el-icon>
              Add（暂存）
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="handlePull"
            >
              <el-icon><Refresh /></el-icon>
              Pull
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="handleFetch"
            >
              <el-icon><Download /></el-icon>
              Fetch
            </el-button>
            <el-button
              size="small"
              :loading="actionLoading"
              @click="openPushDialog"
            >
              <el-icon><Upload /></el-icon>
              Push
            </el-button>
            <el-button
              size="small"
              type="danger"
              plain
              :loading="actionLoading"
              :disabled="selectedFileCount === 0"
              @click="handleRestore"
            >
              <el-icon><RefreshLeft /></el-icon>
              回退
            </el-button>
          </el-button-group>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewGraph(selectedRepoPath)"
          >
            <el-icon><Share /></el-icon>
            Graph
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewDiff(selectedRepoPath)"
          >
            <el-icon><View /></el-icon>
            Diff
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewBranches(selectedRepoPath)"
          >
            <el-icon><Grid /></el-icon>
            分支
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewStash(selectedRepoPath)"
          >
            <el-icon><Box /></el-icon>
            Stash
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedRepoPath"
            @click="viewWorktrees(selectedRepoPath)"
          >
            <el-icon><Files /></el-icon>
            Worktree
          </el-button>
          <el-button
            size="small"
            :disabled="!selectedWorkspaceId"
            @click="viewConflicts"
          >
            <el-icon><Warning /></el-icon>
            冲突
          </el-button>
        </div>
        <div class="commit-row">
          <div class="commit-input">
            <el-input
              v-model="commitForm.message"
              type="textarea"
              :rows="2"
              placeholder="请输入 commit message"
              :disabled="selectedFileCount === 0"
            />
          </div>
          <div class="commit-scope">
            <div
              v-for="(files, repoPath) in selectedFilesByRepo"
              :key="repoPath"
              class="scope-item"
            >
              <span class="scope-repo">{{ repoNameOf(repoPath) }}</span>
              <span class="scope-count">（{{ files.length }} 个文件）</span>
            </div>
          </div>
          <el-button
            type="primary"
            :loading="actionLoading"
            :disabled="selectedFileCount === 0"
            @click="handleCommit"
          >
            <el-icon><EditPen /></el-icon>
            提交
          </el-button>
        </div>
        <!-- Commit options (T-11) -->
        <div class="commit-options">
          <el-checkbox v-model="commitForm.amend" size="small">
            Amend 上次提交
          </el-checkbox>
          <el-checkbox v-model="commitForm.thenPush" size="small">
            提交后 Push
          </el-checkbox>
          <el-button
            size="small"
            text
            :disabled="!selectedRepoPath"
            @click="openIdentityDialog"
          >
            提交身份
          </el-button>
        </div>
      </div>
    </div>

    <!-- Commit identity dialog (T-11 §54) -->
    <el-dialog v-model="identityDialog.show" title="提交身份" width="480px">
      <div class="identity-current">
        当前生效：
        <template v-if="identityDialog.current">
          <strong>
            {{ identityDialog.current.name }} &lt;{{ identityDialog.current.email }}&gt;
          </strong>
          <el-tag size="small" style="margin-left: 6px">
            {{ identitySourceLabel }}
          </el-tag>
        </template>
        <el-tag v-else size="small" type="info">
          Git 默认（user.name / user.email）
        </el-tag>
      </div>
      <el-form label-width="70px" style="margin-top: 12px">
        <el-form-item label="作用于">
          <el-radio-group v-model="identityDialog.scope">
            <el-radio value="repo">本仓库</el-radio>
            <el-radio value="group" :disabled="identityDialog.groupId == null">
              本分组
            </el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item label="Name">
          <el-input v-model="identityDialog.name" placeholder="留空并保存 = 清除自定义" />
        </el-form-item>
        <el-form-item label="Email">
          <el-input v-model="identityDialog.email" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="identityDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="identityDialog.saving"
          @click="saveIdentity"
        >
          保存
        </el-button>
      </template>
    </el-dialog>

    <!-- Pre-commit safety findings dialog (T-11 §5) -->
    <el-dialog v-model="scanDialog.show" title="提交安全检查" width="560px">
      <el-alert
        type="warning"
        :closable="false"
        show-icon
        title="发现以下风险项，确认无误后可放行提交："
      />
      <ul class="scan-finding-list">
        <li v-for="(f, i) in scanDialog.findings" :key="i">
          <el-tag
            size="small"
            :type="f.kind === 'forbidden' ? 'danger' : 'warning'"
          >
            {{ f.kind }}
          </el-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <el-button @click="scanDialog.show = false">取消</el-button>
        <el-button type="danger" @click="commitWithOverride">
          仍要提交
        </el-button>
      </template>
    </el-dialog>

    <!-- Add workspace dialog -->
    <WorkspaceManager v-model="showAddWorkspace" @added="onWorkspaceAdded" />
    <LogManager v-model="showLogManager" />

    <div class="app-footer">by mantougg · v0.1.0</div>

    <!-- Push repo picker dialog -->
    <el-dialog v-model="showPushDialog" title="选择要 Push 的仓库" width="680px">
      <el-table
        ref="pushTableRef"
        :data="changes"
        @selection-change="onPushSelectionChange"
        height="360px"
      >
        <el-table-column type="selection" width="40" />
        <el-table-column label="仓库" min-width="220">
          <template #default="{ row }">
            <div class="push-repo-cell">
              <span class="push-repo-name">{{ row.repoName }}</span>
              <span class="push-repo-rel">{{ row.relativePath }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="分支" width="180">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">{{ row.branch }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="待推送" width="120" align="center">
          <template #default="{ row }">
            <el-tag
              v-if="row.ahead > 0"
              type="warning"
              size="small"
            >
              ↑{{ row.ahead }} 个提交
            </el-tag>
            <span v-else class="text-muted">已同步</span>
          </template>
        </el-table-column>
        <el-table-column label="变更" width="70" align="center">
          <template #default="{ row }">{{ row.changes.length }}</template>
        </el-table-column>
      </el-table>
      <template #footer>
        <el-button @click="showPushDialog = false">取消</el-button>
        <el-button
          type="primary"
          :loading="actionLoading"
          :disabled="pushSelection.length === 0"
          @click="doPush"
        >
          Push（{{ pushSelection.length }}）
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import {
  Plus,
  Refresh,
  Search,
  Bell,
  Monitor,
  CirclePlus,
  Upload,
  EditPen,
  Share,
  View,
  Close,
  ArrowDown,
  ArrowUp,
  Expand,
  Fold,
  RefreshLeft,
  Download,
  FolderOpened,
  Grid,
  Box,
  Files,
  Warning,
} from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { listen } from "@tauri-apps/api/event";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useTaskStore } from "@/stores/task";
import { startWatcher, stopWatcher, batchCommit, batchFetch, batchPull, batchPush } from "@/api/git_ops";
import {
  scanCommit,
  getCommitIdentity,
  setRepoIdentity,
  setGroupIdentity,
} from "@/api/commit";
import type { CommitScanFinding, CommitIdentity } from "@/types/commit";
import { getDiff } from "@/api/git";
import { batchAdd, batchRestore, getWorkspaceChanges, type AddRequest, type RestoreRequest } from "@/api/changes";
import type { CommitRequest } from "@/types/task";
import type { RepoChanges } from "@/types/changes";
import type { ScanProgress } from "@/types/events";
import type { FileDiff } from "@/types/git";
import ChangeTree, {
  type ChangeNode,
  type TreeSelection,
} from "@/components/repo/ChangeTree.vue";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import WorkspaceManager from "@/components/common/WorkspaceManager.vue";
import LogManager from "@/components/common/LogManager.vue";
import { errMsg } from "@/utils/error";

interface SelectedDiff {
  repoPath: string;
  relPath: string;
  file: FileDiff;
}

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();
const taskStore = useTaskStore();

const selectedWorkspaceId = ref<number | null>(null);
const showAddWorkspace = ref(false);
const showLogManager = ref(false);
const watcherActive = ref(false);
const searchQuery = ref("");
const changes = ref<RepoChanges[]>([]);
const changesLoading = ref(false);
const actionLoading = ref(false);
const commitPanelOpen = ref(true);
const commitForm = ref({ message: "", amend: false, thenPush: false });
const scanDialog = ref<{
  show: boolean;
  findings: CommitScanFinding[];
  pending: CommitRequest[];
}>({ show: false, findings: [], pending: [] });
const identityDialog = ref({
  show: false,
  saving: false,
  scope: "repo" as "repo" | "group",
  name: "",
  email: "",
  current: null as CommitIdentity | null,
  groupId: null as number | null,
});
const scanProgress = ref<{ found: number; current: number; total: number | null } | null>(null);
const selectedDiff = ref<SelectedDiff | null>(null);
const diffLoading = ref(false);

const treeSelection = ref<TreeSelection>({ repoPaths: [], filesByRepo: new Map() });
const changeTreeRef = ref<InstanceType<typeof ChangeTree> | null>(null);
const showPushDialog = ref(false);
const pushSelection = ref<string[]>([]);
const pushTableRef = ref();
const diffWidth = ref<number | null>(null);
const diffPaneEl = ref<HTMLElement | null>(null);
let resizeStartX = 0;
let resizeStartWidth = 0;

let unlistenScan: (() => void) | null = null;

const scanPercentage = computed(() => {
  if (!scanProgress.value || !scanProgress.value.total) return 0;
  return Math.round((scanProgress.value.current / scanProgress.value.total) * 100);
});

const dirtyRepoCount = computed(
  () => changes.value.filter((c) => c.changes.length > 0).length,
);

const totalChangedFiles = computed(() =>
  changes.value.reduce((sum, c) => sum + c.changes.length, 0),
);

const selectedRepoCount = computed(() => treeSelection.value.repoPaths.length);

const selectedFileCount = computed(() => {
  let n = 0;
  for (const files of treeSelection.value.filesByRepo.values()) n += files.length;
  return n;
});

const selectedFilesByRepo = computed(() => {
  const result: Record<string, string[]> = {};
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    result[repoPath] = files;
  }
  return result;
});

const selectedRepoPath = computed(() =>
  treeSelection.value.repoPaths.length > 0
    ? treeSelection.value.repoPaths[0]
    : "",
);

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    await loadChanges();
    await startFileWatcher();
  }

  // Listen for scan progress events
  unlistenScan = await listen<ScanProgress>("scan_progress", (event) => {
    scanProgress.value = event.payload;
  });
});

onUnmounted(() => {
  if (unlistenScan) {
    unlistenScan();
    unlistenScan = null;
  }
});

async function loadChanges() {
  if (!selectedWorkspaceId.value) return;
  changesLoading.value = true;
  try {
    changes.value = await getWorkspaceChanges(selectedWorkspaceId.value);
  } catch (e) {
    ElMessage.error("加载变更失败: " + errMsg(e));
  } finally {
    changesLoading.value = false;
  }
}

function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  loadChanges();
  selectedDiff.value = null;
}

function onTreeSelection(selection: TreeSelection) {
  treeSelection.value = selection;
}

function repoNameOf(repoPath: string): string {
  const repo = changes.value.find((c) => c.repoPath === repoPath);
  return repo?.repoName ?? repoPath.split(/[\\/]/).pop() ?? repoPath;
}

/** Double-click a file node: show its change content on the right. */
async function onFileDblClick(node: ChangeNode) {
  if (!node.repoPath || !node.relPath) return;
  diffLoading.value = true;
  try {
    const files = await getDiff(node.repoPath);
    const match = files.find(
      (f) => f.newPath === node.relPath || f.oldPath === node.relPath,
    );
    if (match) {
      selectedDiff.value = {
        repoPath: node.repoPath,
        relPath: node.relPath,
        file: match,
      };
    } else {
      selectedDiff.value = null;
      ElMessage.info("该文件没有可展示的变更内容");
    }
  } catch (e) {
    ElMessage.error("加载变更内容失败: " + errMsg(e));
  } finally {
    diffLoading.value = false;
  }
}

function statusText(status: string): string {
  const map: Record<string, string> = {
    untracked: "未跟踪",
    modified: "已修改",
    deleted: "已删除",
    added: "新增",
    renamed: "重命名",
    typechange: "类型变更",
  };
  return map[status] ?? status;
}

/** Start dragging the diff-pane width. */
function startResize(e: MouseEvent) {
  e.preventDefault();
  resizeStartX = e.clientX;
  resizeStartWidth = diffPaneEl.value?.offsetWidth ?? 600;
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", endResize);
}

function onResizeMove(e: MouseEvent) {
  const delta = resizeStartX - e.clientX; // drag left -> wider diff
  const maxW = window.innerWidth * 0.7;
  diffWidth.value = Math.max(320, Math.min(maxW, resizeStartWidth + delta));
}

function endResize() {
  document.removeEventListener("mousemove", onResizeMove);
  document.removeEventListener("mouseup", endResize);
}

async function handleScan() {
  if (!selectedWorkspaceId.value) return;
  scanProgress.value = null;
  try {
    await repoStore.scanRepositories(selectedWorkspaceId.value);
    ElMessage.success(`发现 ${repoStore.totalCount} 个仓库`);
    await loadChanges();
    await startFileWatcher();
  } catch (e) {
    ElMessage.error("扫描失败: " + errMsg(e));
  } finally {
    scanProgress.value = null;
  }
}

function onWorkspaceAdded() {
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    handleScan();
  }
}

async function handleAdd() {
  const requests: AddRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    requests.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      files,
    });
  }
  if (requests.length === 0) {
    ElMessage.warning("请先勾选要暂存的文件");
    return;
  }
  actionLoading.value = true;
  try {
    await batchAdd(requests);
    ElMessage.success(`已暂存 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("暂存失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

/** Revert working-tree changes for the checked files (with confirmation). */
async function handleRestore() {
  const requests: RestoreRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    requests.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      files,
    });
  }
  if (requests.length === 0) {
    ElMessage.warning("请先勾选要回退的文件");
    return;
  }
  try {
    await ElMessageBox.confirm(
      `确定要回退 ${selectedFileCount.value} 个文件的工作区修改吗？\n已跟踪文件将恢复到 Git 已提交版本（同时取消暂存），未跟踪/新增文件将被删除。`,
      "批量回退",
      { type: "warning", confirmButtonText: "回退", cancelButtonText: "取消" },
    );
  } catch {
    return; // cancelled
  }
  actionLoading.value = true;
  try {
    await batchRestore(requests);
    ElMessage.success(`已回退 ${requests.length} 个仓库的文件`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("回退失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handleCommit() {
  const amend = commitForm.value.amend;
  const message = commitForm.value.message.trim();
  if (!message && !amend) {
    ElMessage.warning("请输入提交信息（Amend 可留空 = --no-edit）");
    return;
  }
  const commits: CommitRequest[] = [];
  for (const [repoPath, files] of treeSelection.value.filesByRepo.entries()) {
    commits.push({
      repoPath,
      repoName: repoNameOf(repoPath),
      message,
      files,
      amend,
      noEdit: amend && !message,
      thenPush: commitForm.value.thenPush,
    });
  }
  if (commits.length === 0) {
    ElMessage.warning("请先勾选要提交的文件");
    return;
  }
  // Pre-commit safety scan (T-11 §5): block on findings until the user
  // explicitly overrides via the findings dialog.
  actionLoading.value = true;
  try {
    const findings: CommitScanFinding[] = [];
    for (const c of commits) {
      findings.push(...(await scanCommit(c.repoPath, c.files, false)));
    }
    if (findings.length > 0) {
      scanDialog.value = { show: true, findings, pending: commits };
      return;
    }
  } catch (e) {
    ElMessage.error("安全检查失败: " + errMsg(e));
    return;
  } finally {
    actionLoading.value = false;
  }
  await submitCommits(commits);
}

/** Resubmit the pending commits with the safety override (T-11 可放行). */
async function commitWithOverride() {
  const commits = scanDialog.value.pending.map((c) => ({
    ...c,
    allowUnsafe: true,
  }));
  scanDialog.value.show = false;
  await submitCommits(commits);
}

async function submitCommits(commits: CommitRequest[]) {
  actionLoading.value = true;
  try {
    const taskIds = await batchCommit(commits);
    ElMessage.success(`已提交 ${taskIds.length} 个 commit 任务`);
    commitForm.value.message = "";
    await loadChanges();
  } catch (e) {
    ElMessage.error("提交失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

const identitySourceLabel = computed(() => {
  switch (identityDialog.value.current?.source) {
    case "repo":
      return "本仓库配置";
    case "group":
      return "分组配置";
    case "mixed":
      return "仓库/分组混合";
    default:
      return "";
  }
});

/** Open the commit-identity dialog for the first selected repo (T-11 §54). */
async function openIdentityDialog() {
  const repo = selectedRepoPath.value;
  if (!repo) return;
  const d = identityDialog.value;
  d.show = true;
  d.scope = "repo";
  d.name = "";
  d.email = "";
  const found = repoStore.repositories.find(
    (r) => r.repository.path === repo,
  );
  d.groupId = found?.repository.groupId ?? null;
  try {
    d.current = await getCommitIdentity(repo);
  } catch {
    d.current = null;
  }
}

/** Save (or clear, when both fields are empty) the identity override. */
async function saveIdentity() {
  const d = identityDialog.value;
  const repo = selectedRepoPath.value;
  if (!repo) return;
  const name = d.name.trim() || null;
  const email = d.email.trim() || null;
  if ((name === null) !== (email === null)) {
    ElMessage.warning("Name 和 Email 需同时填写或同时留空");
    return;
  }
  d.saving = true;
  try {
    if (d.scope === "group" && d.groupId != null) {
      await setGroupIdentity(d.groupId, name, email);
    } else {
      await setRepoIdentity(repo, name, email);
    }
    ElMessage.success(name ? "已保存提交身份" : "已清除自定义身份（恢复默认）");
    d.show = false;
  } catch (e) {
    ElMessage.error("保存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

async function handleFetch() {
  // Selected repos if any; otherwise batch over ALL repositories.
  let paths = treeSelection.value.repoPaths;
  if (paths.length === 0) {
    paths = changes.value.map((c) => c.repoPath);
  }
  if (paths.length === 0) {
    ElMessage.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchFetch(paths);
    ElMessage.success(`已提交 ${taskIds.length} 个 fetch 任务`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("fetch 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

async function handlePull() {
  // Selected repos if any; otherwise batch over ALL repositories.
  let paths = treeSelection.value.repoPaths;
  if (paths.length === 0) {
    paths = changes.value.map((c) => c.repoPath);
  }
  if (paths.length === 0) {
    ElMessage.warning("没有可操作的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPull(paths);
    ElMessage.success(`已提交 ${taskIds.length} 个 pull 任务`);
    await loadChanges();
  } catch (e) {
    ElMessage.error("pull 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

/** Open the push picker dialog, defaulting to the current selection or all repos. */
function openPushDialog() {
  const defaultSelected =
    treeSelection.value.repoPaths.length > 0
      ? treeSelection.value.repoPaths
      : changes.value.map((c) => c.repoPath);
  pushSelection.value = defaultSelected;
  showPushDialog.value = true;
  // Pre-check matching rows after the dialog/table renders.
  setTimeout(() => {
    const table = pushTableRef.value;
    if (!table) return;
    changes.value.forEach((row) => {
      table.toggleRowSelection(row, defaultSelected.includes(row.repoPath));
    });
  }, 50);
}

function onPushSelectionChange(rows: RepoChanges[]) {
  pushSelection.value = rows.map((r) => r.repoPath);
}

async function doPush() {
  if (pushSelection.value.length === 0) {
    ElMessage.warning("请选择要 Push 的仓库");
    return;
  }
  actionLoading.value = true;
  try {
    const taskIds = await batchPush(pushSelection.value);
    ElMessage.success(`已提交 ${taskIds.length} 个 push 任务`);
    showPushDialog.value = false;
    await loadChanges();
  } catch (e) {
    ElMessage.error("push 失败: " + errMsg(e));
  } finally {
    actionLoading.value = false;
  }
}

function expandAll() {
  changeTreeRef.value?.expandAll();
}

function collapseAll() {
  changeTreeRef.value?.collapseAll();
}

async function startFileWatcher() {
  const paths = changes.value.map((c) => c.repoPath);
  if (paths.length === 0) return;
  try {
    await startWatcher(paths);
    watcherActive.value = true;
  } catch (e) {
    console.error("Failed to start watcher:", e);
  }
}

async function toggleWatcher() {
  if (watcherActive.value) {
    try {
      await stopWatcher();
      watcherActive.value = false;
      ElMessage.info("文件监听已停止");
    } catch (e) {
      ElMessage.error("停止监听失败: " + errMsg(e));
    }
  } else {
    await startFileWatcher();
    if (watcherActive.value) {
      ElMessage.success("文件监听已启动");
    }
  }
}

function viewDiff(repoPath: string) {
  router.push({ name: "diff-viewer", query: { repo: repoPath } });
}

function viewGraph(repoPath: string) {
  router.push({ name: "git-graph", query: { repo: repoPath } });
}

function viewBranches(repoPath: string) {
  router.push({ name: "branch-manager", query: { repo: repoPath } });
}

function viewStash(repoPath: string) {
  router.push({ name: "stash-manager", query: { repo: repoPath } });
}

/** Open the worktree manager for the given repo (T-17). */
function viewWorktrees(repo: string) {
  router.push({ name: "worktree-manager", query: { repo } });
}

function viewConflicts() {
  const ws = workspaceStore.currentWorkspace;
  if (!ws) return;
  router.push({
    name: "conflict-resolver",
    query: { workspace: String(ws.id), name: ws.name },
  });
}
</script>

<style scoped>
.repository-list {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  gap: 8px;
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

.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
}

.main-body {
  display: flex;
  flex: 1;
  overflow: hidden;
  gap: 12px;
  min-height: 0;
}

.tree-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.resize-handle {
  width: 5px;
  flex-shrink: 0;
  cursor: col-resize;
  background: transparent;
  border-radius: 2px;
  margin: 0 -1px;
  transition: background 0.15s;
}

.resize-handle:hover {
  background: #409eff;
}

.diff-pane {
  width: 46%;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  overflow: hidden;
}

.diff-pane-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-bottom: 1px solid #ebeef5;
  background: #fafafa;
}

.diff-pane-title {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.diff-repo {
  font-weight: 600;
  color: #409eff;
  font-size: 13px;
  flex-shrink: 0;
}

.diff-file {
  font-family: monospace;
  font-size: 12px;
  color: #606266;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diff-pane-body {
  flex: 1;
  overflow: auto;
  padding: 4px 0;
}

.stats-bar {
  font-size: 13px;
  color: #606266;
  padding: 4px 0;
  display: flex;
  align-items: center;
  flex-wrap: wrap;
}

.tree-controls {
  margin-left: auto;
  display: inline-flex;
  gap: 4px;
}

.selected-info {
  color: #409eff;
  font-weight: 500;
}

.scan-progress {
  margin-bottom: 8px;
  padding: 0 4px;
}

.tree-container {
  flex: 1;
  overflow: hidden;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  min-height: 0;
}

.commit-panel {
  border: 1px solid #ebeef5;
  border-radius: 4px;
  background: #fafafa;
  flex-shrink: 0;
}

.commit-panel-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 8px;
}

.commit-panel-hint {
  font-size: 12px;
  color: #909399;
}

.commit-panel-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 0 12px 10px 12px;
}

.ops-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.commit-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.commit-input {
  flex: 1;
  min-width: 0;
}

.commit-scope {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  max-height: 64px;
  overflow-y: auto;
  max-width: 40%;
}

.scope-repo {
  font-weight: 600;
  color: #409eff;
  font-size: 12px;
}

.scope-count {
  color: #909399;
  font-size: 12px;
}

.push-repo-cell {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.push-repo-name {
  font-weight: 600;
  font-size: 13px;
}

.push-repo-rel {
  font-size: 12px;
  color: #909399;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.separator {
  margin: 0 8px;
  color: #dcdfe6;
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 100%;
}

.app-footer {
  text-align: right;
  font-size: 12px;
  color: #c0c4cc;
  padding: 2px 4px 0;
  flex-shrink: 0;
}

.commit-options {
  display: flex;
  align-items: center;
  gap: 16px;
  margin-top: 6px;
}

.identity-current {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
}

.scan-finding-list {
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
  max-height: 300px;
  overflow-y: auto;
}

.scan-finding-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}

.scan-path {
  font-family: monospace;
  color: #303133;
}

.scan-detail {
  color: #909399;
  font-size: 12px;
}
</style>
