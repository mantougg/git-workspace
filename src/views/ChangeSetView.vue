<template>
  <div class="change-set-view">
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
        <el-button
          type="primary"
          :disabled="!selectedWorkspaceId"
          @click="openCreateDialog"
        >
          <el-icon><Plus /></el-icon>
          新建 Change Set
        </el-button>
        <el-button
          :disabled="!selectedWorkspaceId"
          :loading="store.loading"
          @click="reloadList"
        >
          <el-icon><Refresh /></el-icon>
          刷新
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button v-if="taskStore.tasks.length > 0" @click="taskStore.togglePanel()">
          任务 ({{ taskStore.tasks.length }})
        </el-button>
      </div>
    </div>

    <div class="main-body">
      <!-- Left: change set list -->
      <div class="set-list" v-loading="store.loading">
        <div
          v-for="cs in store.changeSets"
          :key="cs.id"
          :class="['set-item', { active: cs.id === store.currentId }]"
          @click="store.selectChangeSet(cs.id)"
        >
          <div class="set-item-title">
            <span class="set-name">{{ cs.name }}</span>
            <el-button
              size="small"
              text
              type="danger"
              :icon="Delete"
              @click.stop="handleDeleteSet(cs)"
            />
          </div>
          <div v-if="cs.description" class="set-desc">{{ cs.description }}</div>
          <div class="set-updated">更新于 {{ formatTime(cs.updatedAt) }}</div>
        </div>
        <el-empty
          v-if="!store.loading && store.changeSets.length === 0"
          description="还没有 Change Set"
          :image-size="60"
        >
          <el-button
            type="primary"
            size="small"
            :disabled="!selectedWorkspaceId"
            @click="openCreateDialog"
          >
            新建 Change Set
          </el-button>
        </el-empty>
      </div>

      <!-- Right: selected change set detail -->
      <div class="set-detail" v-loading="store.summaryLoading">
        <template v-if="summary">
          <div class="detail-header">
            <div class="detail-title">
              <span class="detail-name">{{ summary.changeSet.name }}</span>
              <el-button size="small" text :icon="Edit" @click="openEditDialog" />
            </div>
            <div v-if="summary.changeSet.description" class="detail-desc">
              {{ summary.changeSet.description }}
            </div>
          </div>

          <!-- Unified summary cards -->
          <div class="stats-cards">
            <div class="stat-card">
              <div class="stat-value">{{ summary.repositories }}</div>
              <div class="stat-label">Repositories</div>
            </div>
            <div class="stat-card">
              <div class="stat-value">{{ summary.files }}</div>
              <div class="stat-label">Files</div>
            </div>
            <div class="stat-card">
              <div class="stat-value added">+{{ summary.added }}</div>
              <div class="stat-label">Added</div>
            </div>
            <div class="stat-card">
              <div class="stat-value deleted">-{{ summary.deleted }}</div>
              <div class="stat-label">Deleted</div>
            </div>
            <div class="stat-card">
              <div class="stat-value">{{ summary.commits }}</div>
              <div class="stat-label">Commits（待推送）</div>
            </div>
          </div>

          <!-- Actions -->
          <div class="action-bar">
            <el-button size="small" type="primary" plain @click="openAddDialog">
              <el-icon><Link /></el-icon>
              添加仓库
            </el-button>
            <el-button
              size="small"
              :disabled="summary.repositories === 0"
              @click="openDiffDialog"
            >
              <el-icon><View /></el-icon>
              View All Diff
            </el-button>
            <el-button
              size="small"
              :disabled="summary.repositories === 0"
              @click="aiPicker.show = true"
            >
              <el-icon><MagicStick /></el-icon>
              AI Review
            </el-button>
            <el-button
              size="small"
              type="primary"
              :disabled="summary.repositories === 0"
              :loading="commitDialog.collecting"
              @click="openCommitDialog"
            >
              <el-icon><EditPen /></el-icon>
              Commit All
            </el-button>
            <el-button
              size="small"
              type="warning"
              plain
              :disabled="summary.repositories === 0"
              @click="pushDialog.show = true"
            >
              <el-icon><Upload /></el-icon>
              Push All
            </el-button>
            <el-tooltip content="Create PRs 由 T-29 实现（尚未开发）" placement="top">
              <span>
                <el-button size="small" disabled>
                  <el-icon><Promotion /></el-icon>
                  Create PRs（T-29）
                </el-button>
              </span>
            </el-tooltip>
            <el-button size="small" text :icon="Refresh" @click="store.refreshSummary()">
              刷新统计
            </el-button>
          </div>

          <!-- Member repo table -->
          <el-table :data="summary.repos" size="small" class="repo-table">
            <el-table-column label="仓库" min-width="160">
              <template #default="{ row }">
                <div class="repo-cell">
                  <span class="repo-name">{{ row.repo.repoName }}</span>
                  <span class="repo-rel">{{ row.repo.relativePath }}</span>
                </div>
              </template>
            </el-table-column>
            <el-table-column label="当前分支" width="130">
              <template #default="{ row }">
                <el-tag size="small" effect="plain">
                  {{ row.currentBranch ?? "—" }}
                </el-tag>
              </template>
            </el-table-column>
            <el-table-column label="目标分支" width="170">
              <template #default="{ row }">
                <span class="target-branch">{{ row.repo.targetBranch ?? "—" }}</span>
                <el-button
                  size="small"
                  text
                  :icon="Edit"
                  @click="openBranchDialog(row as ChangeSetRepoSummary)"
                />
              </template>
            </el-table-column>
            <el-table-column label="前/后" width="80" align="center">
              <template #default="{ row }">
                <span v-if="row.ahead > 0" class="ahead">↑{{ row.ahead }}</span>
                <span v-if="row.behind > 0" class="behind">↓{{ row.behind }}</span>
                <span v-if="row.ahead === 0 && row.behind === 0">—</span>
              </template>
            </el-table-column>
            <el-table-column prop="files" label="Files" width="70" align="center" />
            <el-table-column label="+/-" width="110" align="center">
              <template #default="{ row }">
                <span class="added">+{{ row.added }}</span>
                <span class="deleted"> -{{ row.deleted }}</span>
              </template>
            </el-table-column>
            <el-table-column label="状态" min-width="120">
              <template #default="{ row }">
                <el-tag v-if="row.error" size="small" type="danger">
                  {{ row.error }}
                </el-tag>
                <el-tag v-else-if="row.files > 0" size="small" type="warning">
                  有变更
                </el-tag>
                <el-tag v-else size="small" type="success">干净</el-tag>
              </template>
            </el-table-column>
            <el-table-column width="70" align="center">
              <template #default="{ row }">
                <el-button
                  size="small"
                  text
                  type="danger"
                  :icon="Delete"
                  @click="handleRemoveRepo(row as ChangeSetRepoSummary)"
                />
              </template>
            </el-table-column>
            <template #empty>
              <el-empty description="尚未关联仓库" :image-size="60" />
            </template>
          </el-table>
        </template>
        <el-empty v-else description="选择左侧的 Change Set，或新建一个" />
      </div>
    </div>

    <!-- Create / edit change set dialog -->
    <el-dialog
      v-model="setDialog.show"
      :title="setDialog.editingId == null ? '新建 Change Set' : '编辑 Change Set'"
      width="480px"
    >
      <el-form label-width="70px">
        <el-form-item label="名称" required>
          <el-input
            v-model="setDialog.name"
            placeholder="如：Feature: AI Review"
            maxlength="80"
          />
        </el-form-item>
        <el-form-item label="描述">
          <el-input v-model="setDialog.description" type="textarea" :rows="3" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="setDialog.show = false">取消</el-button>
        <el-button type="primary" :loading="setDialog.saving" @click="saveSet">
          {{ setDialog.editingId == null ? "创建" : "保存" }}
        </el-button>
      </template>
    </el-dialog>

    <!-- Add repositories dialog (T-20 selector 联动) -->
    <el-dialog v-model="addDialog.show" title="添加仓库到 Change Set" width="780px">
      <div class="selector-row">
        <el-input
          v-model="addDialog.selector"
          size="small"
          placeholder="选择器：@group:frontend @tag:p0 @status:dirty 或名称关键字（T-20）"
          clearable
          style="flex: 1"
          @keyup.enter="applySelector"
        />
        <el-button
          size="small"
          :loading="addDialog.selectorLoading"
          :disabled="!addDialog.selector.trim()"
          @click="applySelector"
        >
          <el-icon><Select /></el-icon>
          按选择器勾选
        </el-button>
      </div>
      <el-table
        ref="addTableRef"
        :data="addDialog.repos"
        size="small"
        height="360px"
        v-loading="addDialog.loading"
        @selection-change="onAddSelectionChange"
      >
        <el-table-column
          type="selection"
          width="40"
          :selectable="(row: RepositoryWithStatus) => !isMember(row as RepositoryWithStatus)"
        />
        <el-table-column label="仓库" min-width="170">
          <template #default="{ row }">
            <div class="repo-cell">
              <span class="repo-name">{{ row.repository.name }}</span>
              <span class="repo-rel">{{ row.repository.relativePath }}</span>
            </div>
            <el-tag v-if="isMember(row as RepositoryWithStatus)" size="small" type="info" effect="plain">
              已关联
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="当前分支" width="120">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">
              {{ row.status?.branch ?? "—" }}
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="目标分支（默认当前分支）" min-width="180">
          <template #default="{ row }">
            <el-input
              v-model="addDialog.branches[row.repository.id ?? -1]"
              size="small"
              placeholder="feature/xxx"
              :disabled="isMember(row as RepositoryWithStatus)"
            />
          </template>
        </el-table-column>
      </el-table>
      <template #footer>
        <el-button @click="addDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="addDialog.saving"
          :disabled="addDialog.checked.length === 0"
          @click="saveAddRepos"
        >
          添加（{{ addDialog.checked.length }}）
        </el-button>
      </template>
    </el-dialog>

    <!-- Target branch edit dialog -->
    <el-dialog v-model="branchDialog.show" title="设置目标分支" width="420px">
      <div class="branch-dialog-body">
        <span class="branch-dialog-repo">{{ branchDialog.repoName }}</span>
        <el-input
          v-model="branchDialog.branch"
          placeholder="留空 = 清除目标分支"
          clearable
        />
      </div>
      <template #footer>
        <el-button @click="branchDialog.show = false">取消</el-button>
        <el-button type="primary" :loading="branchDialog.saving" @click="saveBranch">
          保存
        </el-button>
      </template>
    </el-dialog>

    <!-- View All Diff dialog（多仓库聚合，按仓库懒加载） -->
    <el-dialog
      v-model="diffDialog.show"
      :title="`View All Diff — ${summary?.changeSet.name ?? ''}`"
      width="92%"
      top="4vh"
      class="all-diff-dialog"
    >
      <div class="all-diff-body">
        <div class="all-diff-repos">
          <div
            v-for="row in summary?.repos ?? []"
            :key="row.repo.repoPath"
            :class="['all-diff-repo', { active: row.repo.repoPath === diffDialog.repoPath }]"
            @click="openDiffRepo(row.repo.repoPath)"
          >
            <span class="repo-name">{{ row.repo.repoName }}</span>
            <el-tag size="small" effect="plain">{{ row.files }} 文件</el-tag>
          </div>
        </div>
        <div class="all-diff-files" v-loading="diffDialog.loading">
          <div
            v-for="file in diffDialog.files"
            :key="file.newPath || file.oldPath"
            :class="['file-item', { active: diffDialog.file === file }]"
            @click="diffDialog.file = file"
          >
            <span :class="['file-status-icon', file.status]">
              {{ statusIcon(file.status) }}
            </span>
            <span class="file-name">{{ file.newPath || file.oldPath }}</span>
          </div>
          <el-empty
            v-if="!diffDialog.loading && diffDialog.repoPath && diffDialog.files.length === 0"
            description="该仓库没有变更"
            :image-size="60"
          />
          <el-empty
            v-else-if="!diffDialog.repoPath"
            description="选择左侧仓库查看 Diff"
            :image-size="60"
          />
        </div>
        <div class="all-diff-content">
          <UnifiedDiff v-if="diffDialog.file" :file="diffDialog.file" />
          <el-empty v-else description="选择文件查看 Diff" :image-size="60" />
        </div>
      </div>
    </el-dialog>

    <!-- AI Review: repo picker -->
    <el-dialog v-model="aiPicker.show" title="AI Review — 选择仓库" width="420px">
      <el-radio-group v-model="aiPicker.repoPath" class="ai-repo-list">
        <el-radio
          v-for="row in summary?.repos ?? []"
          :key="row.repo.repoPath"
          :value="row.repo.repoPath"
        >
          {{ row.repo.repoName }}（{{ row.files }} 个文件变更）
        </el-radio>
      </el-radio-group>
      <template #footer>
        <el-button @click="aiPicker.show = false">取消</el-button>
        <el-button
          type="primary"
          :disabled="!aiPicker.repoPath"
          :loading="aiPicker.loading"
          @click="startAiReview"
        >
          开始审查
        </el-button>
      </template>
    </el-dialog>

    <!-- AI Review result -->
    <el-dialog
      v-model="aiPicker.showResult"
      title="AI Code Review"
      width="600px"
      :close-on-click-modal="false"
    >
      <div v-if="aiPicker.result" class="review-result">
        <div class="review-summary">
          <strong>Summary:</strong> {{ aiPicker.result.summary }}
        </div>
        <div v-if="aiPicker.result.issues.length > 0" class="review-issues">
          <div
            v-for="(issue, i) in aiPicker.result.issues"
            :key="i"
            class="review-issue"
          >
            <el-tag
              :type="issue.severity === 'high' ? 'danger' : issue.severity === 'medium' ? 'warning' : 'info'"
              size="small"
            >
              {{ issue.severity }}
            </el-tag>
            <el-tag size="small" effect="plain">{{ issue.category }}</el-tag>
            <span class="issue-file">{{ issue.file }}</span>
            <div class="issue-desc">{{ issue.description }}</div>
          </div>
        </div>
        <el-empty v-else description="No issues found" :image-size="60" />
      </div>
    </el-dialog>

    <!-- Commit All dialog -->
    <el-dialog v-model="commitDialog.show" title="Commit All（批量提交）" width="560px">
      <el-input
        v-model="commitDialog.message"
        type="textarea"
        :rows="3"
        placeholder="请输入 commit message（作用于以下所有仓库）"
      />
      <el-checkbox v-model="commitDialog.thenPush" size="small" style="margin-top: 8px">
        提交后 Push
      </el-checkbox>
      <div class="commit-scope">
        <div
          v-for="scope in commitDialog.scopes"
          :key="scope.repoPath"
          class="scope-item"
        >
          <span class="scope-repo">{{ scope.repoName }}</span>
          <span class="scope-count">（{{ scope.files.length }} 个文件）</span>
        </div>
        <div v-if="commitDialog.skipped.length > 0" class="scope-skipped">
          跳过无变更仓库：{{ commitDialog.skipped.join("、") }}
        </div>
      </div>
      <template #footer>
        <el-button @click="commitDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="commitDialog.submitting"
          :disabled="commitDialog.scopes.length === 0"
          @click="handleCommitAll"
        >
          提交（{{ commitDialog.scopes.length }} 个仓库）
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
          <el-tag size="small" :type="f.kind === 'forbidden' ? 'danger' : 'warning'">
            {{ f.kind }}
          </el-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <el-button @click="scanDialog.show = false">取消</el-button>
        <el-button type="danger" @click="commitAllWithOverride">仍要提交</el-button>
      </template>
    </el-dialog>

    <!-- Push All confirm dialog -->
    <el-dialog v-model="pushDialog.show" title="Push All（批量推送）" width="520px">
      <template v-if="pushCandidates.length > 0">
        <el-alert
          type="info"
          :closable="false"
          show-icon
          :title="`将推送以下 ${pushCandidates.length} 个仓库的当前分支`"
        />
        <ul class="push-repo-list">
          <li v-for="row in pushCandidates" :key="row.repo.repoPath">
            <span class="repo-name">{{ row.repo.repoName }}</span>
            <el-tag size="small" effect="plain">{{ row.currentBranch }}</el-tag>
            <span class="ahead">↑{{ row.ahead }}</span>
          </li>
        </ul>
        <div v-if="pushSkipped.length > 0" class="scope-skipped">
          无需推送（本地不领先）：{{ pushSkipped.join("、") }}
        </div>
      </template>
      <el-empty v-else description="所有关联仓库均已同步（无待推送提交）" :image-size="60" />
      <template #footer>
        <el-button @click="pushDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="pushDialog.loading"
          :disabled="pushCandidates.length === 0"
          @click="doPushAll"
        >
          Push（{{ pushCandidates.length }}）
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  Plus,
  Refresh,
  View,
  MagicStick,
  EditPen,
  Upload,
  Delete,
  Edit,
  Link,
  Promotion,
  Select,
} from "@element-plus/icons-vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { useTaskStore } from "@/stores/task";
import { useChangeSetStore } from "@/stores/changeSet";
import { listRepositories } from "@/api/repository";
import { selectRepos } from "@/api/batch";
import { getDiff } from "@/api/git";
import { aiReview } from "@/api/ai";
import { batchCommit, batchPush } from "@/api/git_ops";
import { scanCommit } from "@/api/commit";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import { errMsg } from "@/utils/error";
import type { ChangeSet, ChangeSetRepoSummary } from "@/types/changeSet";
import type { RepositoryWithStatus } from "@/types/repository";
import type { ReviewResult } from "@/types/ai";
import type { CommitRequest, TaskProgress } from "@/types/task";
import type { CommitScanFinding } from "@/types/commit";
import type { FileDiff } from "@/types/git";

const workspaceStore = useWorkspaceStore();
const taskStore = useTaskStore();
const store = useChangeSetStore();

const selectedWorkspaceId = ref<number | null>(null);
const summary = computed(() => store.summary);

// --- Create / edit change set ---
const setDialog = ref({
  show: false,
  saving: false,
  editingId: null as number | null,
  name: "",
  description: "",
});

// --- Add repositories ---
const addDialog = ref({
  show: false,
  loading: false,
  saving: false,
  selectorLoading: false,
  selector: "",
  repos: [] as RepositoryWithStatus[],
  checked: [] as RepositoryWithStatus[],
  branches: {} as Record<number, string>,
});
const addTableRef = ref();

// --- Target branch edit ---
const branchDialog = ref({
  show: false,
  saving: false,
  repoId: 0,
  repoName: "",
  branch: "",
});

// --- View All Diff ---
const diffDialog = ref({
  show: false,
  loading: false,
  repoPath: null as string | null,
  files: [] as FileDiff[],
  file: null as FileDiff | null,
});
const diffCache = new Map<string, FileDiff[]>();

// --- AI Review ---
const aiPicker = ref({
  show: false,
  loading: false,
  repoPath: "",
  showResult: false,
  result: null as ReviewResult | null,
});

// --- Commit All ---
const commitDialog = ref({
  show: false,
  collecting: false,
  submitting: false,
  message: "",
  thenPush: false,
  scopes: [] as { repoPath: string; repoName: string; files: string[] }[],
  skipped: [] as string[],
});
const scanDialog = ref({
  show: false,
  findings: [] as CommitScanFinding[],
});

// --- Push All ---
const pushDialog = ref({ show: false, loading: false });

const pushCandidates = computed(() =>
  (summary.value?.repos ?? []).filter((r) => r.ahead > 0),
);
const pushSkipped = computed(() =>
  (summary.value?.repos ?? [])
    .filter((r) => r.ahead === 0)
    .map((r) => r.repo.repoName),
);

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    await loadCurrentList();
  }
  // Refresh the summary once batch commit/push tasks reach a final state
  // (debounced: a batch emits one event per repo).
  unlistenTasks = await listen<TaskProgress>("task_progress", (e) => {
    const kind = e.payload.taskType.type;
    if (kind !== "commit" && kind !== "push") return;
    const statusType = e.payload.status.type;
    const final =
      statusType === "success" ||
      statusType === "failed" ||
      statusType === "partialSuccess" ||
      statusType === "cancelled";
    if (!final || store.currentId == null) return;
    window.clearTimeout(refreshTimer);
    refreshTimer = window.setTimeout(() => {
      store.refreshSummary().catch(() => {});
      if (selectedWorkspaceId.value) {
        store.loadChangeSets(selectedWorkspaceId.value).catch(() => {});
      }
    }, 800);
  });
});

let unlistenTasks: (() => void) | null = null;
let refreshTimer: number | undefined;

onUnmounted(() => {
  if (unlistenTasks) {
    unlistenTasks();
    unlistenTasks = null;
  }
  window.clearTimeout(refreshTimer);
});

// Helper so onMounted stays linear (load list for the picked workspace).
async function loadCurrentList() {
  if (selectedWorkspaceId.value) {
    await store.loadChangeSets(selectedWorkspaceId.value);
  }
}

function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  store.selectChangeSet(null);
  loadCurrentList();
}

function reloadList() {
  loadCurrentList();
  if (store.currentId != null) {
    store.refreshSummary();
  }
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

// ---------------------------------------------------------------------------
// Change set CRUD
// ---------------------------------------------------------------------------

function openCreateDialog() {
  setDialog.value = { show: true, saving: false, editingId: null, name: "", description: "" };
}

function openEditDialog() {
  const cs = summary.value?.changeSet;
  if (!cs) return;
  setDialog.value = {
    show: true,
    saving: false,
    editingId: cs.id,
    name: cs.name,
    description: cs.description ?? "",
  };
}

async function saveSet() {
  const d = setDialog.value;
  const name = d.name.trim();
  if (!name) {
    ElMessage.warning("请输入名称");
    return;
  }
  d.saving = true;
  try {
    if (d.editingId == null) {
      const cs = await store.createSet({
        workspaceId: selectedWorkspaceId.value!,
        name,
        description: d.description.trim() || null,
      });
      ElMessage.success("已创建 Change Set");
      d.show = false;
      await store.selectChangeSet(cs.id);
    } else {
      await store.updateSet({
        id: d.editingId,
        name,
        description: d.description.trim() || null,
      });
      ElMessage.success("已保存");
      d.show = false;
    }
  } catch (e) {
    ElMessage.error("保存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

async function handleDeleteSet(cs: ChangeSet) {
  try {
    await ElMessageBox.confirm(
      `确定删除 Change Set「${cs.name}」吗？仅删除关联关系，不影响任何仓库代码。`,
      "删除确认",
      { type: "warning", confirmButtonText: "删除", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }
  try {
    await store.removeSet(cs.id);
    ElMessage.success("已删除");
  } catch (e) {
    ElMessage.error("删除失败: " + errMsg(e));
  }
}

// ---------------------------------------------------------------------------
// Repo association
// ---------------------------------------------------------------------------

function isMember(row: RepositoryWithStatus): boolean {
  const id = row.repository.id;
  return (
    id != null &&
    (summary.value?.repos ?? []).some((r) => r.repo.repoId === id)
  );
}

async function openAddDialog() {
  if (!selectedWorkspaceId.value) return;
  addDialog.value.show = true;
  addDialog.value.loading = true;
  addDialog.value.selector = "";
  addDialog.value.checked = [];
  try {
    const repos = await listRepositories(selectedWorkspaceId.value);
    addDialog.value.repos = repos;
    const branches: Record<number, string> = {};
    for (const r of repos) {
      const id = r.repository.id;
      if (id != null) branches[id] = r.status?.branch ?? "";
    }
    addDialog.value.branches = branches;
    await nextTick();
    addTableRef.value?.clearSelection();
  } catch (e) {
    ElMessage.error("加载仓库列表失败: " + errMsg(e));
  } finally {
    addDialog.value.loading = false;
  }
}

function onAddSelectionChange(rows: RepositoryWithStatus[]) {
  addDialog.value.checked = rows;
}

/** T-20 联动：用选择器结果勾选仓库。 */
async function applySelector() {
  const query = addDialog.value.selector.trim();
  if (!query || !selectedWorkspaceId.value) return;
  addDialog.value.selectorLoading = true;
  try {
    const paths = await selectRepos(selectedWorkspaceId.value, query);
    const matched = new Set(paths);
    const table = addTableRef.value;
    if (!table) return;
    table.clearSelection();
    for (const row of addDialog.value.repos) {
      if (matched.has(row.repository.path) && !isMember(row as RepositoryWithStatus)) {
        table.toggleRowSelection(row, true);
      }
    }
    ElMessage.success(`选择器命中 ${paths.length} 个仓库，已勾选可添加项`);
  } catch (e) {
    ElMessage.error("选择器查询失败: " + errMsg(e));
  } finally {
    addDialog.value.selectorLoading = false;
  }
}

async function saveAddRepos() {
  const csId = store.currentId;
  if (csId == null) return;
  const inputs = addDialog.value.checked
    .map((r) => r.repository.id)
    .filter((id): id is number => id != null)
    .map((repoId) => ({
      repoId,
      targetBranch: addDialog.value.branches[repoId]?.trim() || null,
    }));
  if (inputs.length === 0) return;
  addDialog.value.saving = true;
  try {
    await store.addRepos(csId, inputs);
    ElMessage.success(`已关联 ${inputs.length} 个仓库`);
    addDialog.value.show = false;
  } catch (e) {
    ElMessage.error("添加失败: " + errMsg(e));
  } finally {
    addDialog.value.saving = false;
  }
}

async function handleRemoveRepo(row: ChangeSetRepoSummary) {
  const csId = store.currentId;
  if (csId == null) return;
  try {
    await store.removeRepo(csId, row.repo.repoId);
    ElMessage.success(`已移除 ${row.repo.repoName}`);
  } catch (e) {
    ElMessage.error("移除失败: " + errMsg(e));
  }
}

function openBranchDialog(row: ChangeSetRepoSummary) {
  branchDialog.value = {
    show: true,
    saving: false,
    repoId: row.repo.repoId,
    repoName: row.repo.repoName,
    branch: row.repo.targetBranch ?? row.currentBranch ?? "",
  };
}

async function saveBranch() {
  const csId = store.currentId;
  if (csId == null) return;
  branchDialog.value.saving = true;
  try {
    // 关联接口本身即 upsert：同 repoId 更新 target_branch。
    await store.addRepos(csId, [
      {
        repoId: branchDialog.value.repoId,
        targetBranch: branchDialog.value.branch.trim() || null,
      },
    ]);
    branchDialog.value.show = false;
    ElMessage.success("已更新目标分支");
  } catch (e) {
    ElMessage.error("保存失败: " + errMsg(e));
  } finally {
    branchDialog.value.saving = false;
  }
}

// ---------------------------------------------------------------------------
// View All Diff（按仓库懒加载 + 结果缓存）
// ---------------------------------------------------------------------------

function openDiffDialog() {
  diffCache.clear();
  diffDialog.value = { show: true, loading: false, repoPath: null, files: [], file: null };
  // 默认打开第一个有变更的仓库。
  const firstDirty = (summary.value?.repos ?? []).find((r) => r.files > 0);
  if (firstDirty) {
    openDiffRepo(firstDirty.repo.repoPath);
  }
}

async function openDiffRepo(repoPath: string) {
  diffDialog.value.repoPath = repoPath;
  diffDialog.value.file = null;
  const cached = diffCache.get(repoPath);
  if (cached) {
    diffDialog.value.files = cached;
    return;
  }
  diffDialog.value.loading = true;
  try {
    const files = await getDiff(repoPath);
    diffCache.set(repoPath, files);
    diffDialog.value.files = files;
  } catch (e) {
    diffDialog.value.files = [];
    ElMessage.error("获取 Diff 失败: " + errMsg(e));
  } finally {
    diffDialog.value.loading = false;
  }
}

function statusIcon(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "A";
    case "deleted":
      return "D";
    case "modified":
      return "M";
    case "renamed":
      return "R";
    default:
      return "?";
  }
}

// ---------------------------------------------------------------------------
// AI Review（入口：逐仓库审查，复用 ai_review command）
// ---------------------------------------------------------------------------

async function startAiReview() {
  const repoPath = aiPicker.value.repoPath;
  if (!repoPath) return;
  try {
    const { value: apiKey } = await ElMessageBox.prompt(
      "请输入您的 AI API Key",
      "AI Code Review",
      {
        confirmButtonText: "开始审查",
        cancelButtonText: "取消",
        inputType: "password",
        inputPlaceholder: "OpenAI API Key",
      },
    );
    if (!apiKey) return;
    aiPicker.value.loading = true;
    aiPicker.value.result = await aiReview(repoPath, apiKey);
    aiPicker.value.show = false;
    aiPicker.value.showResult = true;
  } catch (e) {
    if (e !== "cancel") {
      ElMessage.error("AI Review 失败: " + errMsg(e));
    }
  } finally {
    aiPicker.value.loading = false;
  }
}

// ---------------------------------------------------------------------------
// Commit All（任务队列 + T-11 提交前安全检查）
// ---------------------------------------------------------------------------

/** Collect the changed-file list per member repo via T-04 get_diff. */
async function openCommitDialog() {
  const rows = summary.value?.repos ?? [];
  if (rows.length === 0) return;
  commitDialog.value.collecting = true;
  try {
    const results = await Promise.all(
      rows.map(async (row) => {
        try {
          const diffs = await getDiff(row.repo.repoPath);
          return {
            repoPath: row.repo.repoPath,
            repoName: row.repo.repoName,
            files: diffs.map((d) => d.newPath || d.oldPath),
          };
        } catch {
          return { repoPath: row.repo.repoPath, repoName: row.repo.repoName, files: [] };
        }
      }),
    );
    const scopes = results.filter((r) => r.files.length > 0);
    if (scopes.length === 0) {
      ElMessage.info("所有关联仓库都没有可提交的变更");
      return;
    }
    commitDialog.value.scopes = scopes;
    commitDialog.value.skipped = results
      .filter((r) => r.files.length === 0)
      .map((r) => r.repoName);
    commitDialog.value.message = "";
    commitDialog.value.thenPush = false;
    commitDialog.value.show = true;
  } finally {
    commitDialog.value.collecting = false;
  }
}

async function handleCommitAll() {
  const message = commitDialog.value.message.trim();
  if (!message) {
    ElMessage.warning("请输入提交信息");
    return;
  }
  // 提交前安全检查（全局约束 §5）：发现风险须用户显式放行。
  commitDialog.value.submitting = true;
  try {
    const findings: CommitScanFinding[] = [];
    for (const scope of commitDialog.value.scopes) {
      findings.push(...(await scanCommit(scope.repoPath, scope.files, false)));
    }
    if (findings.length > 0) {
      scanDialog.value = { show: true, findings };
      return;
    }
  } catch (e) {
    ElMessage.error("安全检查失败: " + errMsg(e));
    return;
  } finally {
    commitDialog.value.submitting = false;
  }
  await submitCommitAll(false);
}

async function commitAllWithOverride() {
  scanDialog.value.show = false;
  await submitCommitAll(true);
}

async function submitCommitAll(allowUnsafe: boolean) {
  const message = commitDialog.value.message.trim();
  const commits: CommitRequest[] = commitDialog.value.scopes.map((scope) => ({
    repoPath: scope.repoPath,
    repoName: scope.repoName,
    message,
    files: scope.files,
    thenPush: commitDialog.value.thenPush,
    allowUnsafe,
  }));
  commitDialog.value.submitting = true;
  try {
    const taskIds = await batchCommit(commits);
    ElMessage.success(`已提交 ${taskIds.length} 个 commit 任务，进度见任务面板`);
    commitDialog.value.show = false;
    taskStore.showPanel();
  } catch (e) {
    ElMessage.error("提交失败: " + errMsg(e));
  } finally {
    commitDialog.value.submitting = false;
  }
}

// ---------------------------------------------------------------------------
// Push All（任务队列；仅推送本地领先的仓库）
// ---------------------------------------------------------------------------

async function doPushAll() {
  const paths = pushCandidates.value.map((r) => r.repo.repoPath);
  if (paths.length === 0) return;
  pushDialog.value.loading = true;
  try {
    const taskIds = await batchPush(paths);
    ElMessage.success(`已提交 ${taskIds.length} 个 push 任务，进度见任务面板`);
    pushDialog.value.show = false;
    taskStore.showPanel();
  } catch (e) {
    ElMessage.error("push 失败: " + errMsg(e));
  } finally {
    pushDialog.value.loading = false;
  }
}
</script>

<style scoped>
.change-set-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  border-bottom: 1px solid #ebeef5;
  background: #fff;
}

.toolbar-left {
  display: flex;
  gap: 8px;
  align-items: center;
}

.main-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.set-list {
  width: 280px;
  border-right: 1px solid #ebeef5;
  overflow-y: auto;
  background: #fafafa;
  padding: 8px;
}

.set-item {
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  border: 1px solid transparent;
  margin-bottom: 6px;
  background: #fff;
}

.set-item:hover {
  border-color: #c6e2ff;
}

.set-item.active {
  border-color: #409eff;
  background: #ecf5ff;
}

.set-item-title {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.set-name {
  font-weight: 600;
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.set-desc {
  font-size: 12px;
  color: #606266;
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.set-updated {
  font-size: 11px;
  color: #909399;
  margin-top: 4px;
}

.set-detail {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}

.detail-header {
  margin-bottom: 12px;
}

.detail-title {
  display: flex;
  align-items: center;
  gap: 4px;
}

.detail-name {
  font-size: 18px;
  font-weight: 600;
}

.detail-desc {
  font-size: 13px;
  color: #606266;
  margin-top: 4px;
}

.stats-cards {
  display: flex;
  gap: 12px;
  margin-bottom: 12px;
}

.stat-card {
  flex: 1;
  background: #fff;
  border: 1px solid #ebeef5;
  border-radius: 6px;
  padding: 12px;
  text-align: center;
}

.stat-value {
  font-size: 22px;
  font-weight: 600;
}

.stat-value.added,
.added {
  color: #67c23a;
}

.stat-value.deleted,
.deleted {
  color: #f56c6c;
}

.stat-label {
  font-size: 12px;
  color: #909399;
  margin-top: 2px;
}

.action-bar {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  margin-bottom: 12px;
}

.action-bar .el-button + .el-button {
  margin-left: 0;
}

.repo-table {
  border: 1px solid #ebeef5;
  border-radius: 6px;
}

.repo-cell {
  display: flex;
  flex-direction: column;
}

.repo-name {
  font-weight: 500;
}

.repo-rel {
  font-size: 11px;
  color: #909399;
}

.target-branch {
  margin-right: 4px;
  font-family: monospace;
  font-size: 12px;
}

.ahead {
  color: #e6a23c;
  margin-right: 4px;
}

.behind {
  color: #409eff;
}

.selector-row {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}

.branch-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.branch-dialog-repo {
  font-weight: 500;
}

.all-diff-body {
  display: flex;
  height: 72vh;
  border: 1px solid #ebeef5;
  border-radius: 6px;
  overflow: hidden;
}

.all-diff-repos {
  width: 200px;
  border-right: 1px solid #ebeef5;
  overflow-y: auto;
  background: #fafafa;
}

.all-diff-repo {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 10px;
  cursor: pointer;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}

.all-diff-repo:hover {
  background: #f5f7fa;
}

.all-diff-repo.active {
  background: #ecf5ff;
  border-left: 3px solid #409eff;
  padding-left: 7px;
}

.all-diff-files {
  width: 260px;
  border-right: 1px solid #ebeef5;
  overflow-y: auto;
}

.all-diff-content {
  flex: 1;
  overflow: hidden;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  cursor: pointer;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}

.file-item:hover {
  background: #f5f7fa;
}

.file-item.active {
  background: #ecf5ff;
  border-left: 3px solid #409eff;
  padding-left: 9px;
}

.file-status-icon {
  width: 16px;
  text-align: center;
  font-weight: bold;
  flex-shrink: 0;
}

.file-status-icon.added,
.file-status-icon.untracked {
  color: #67c23a;
}

.file-status-icon.deleted {
  color: #f56c6c;
}

.file-status-icon.modified {
  color: #e6a23c;
}

.file-status-icon.renamed {
  color: #909399;
}

.file-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ai-repo-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.review-result {
  padding: 4px;
}

.review-summary {
  margin-bottom: 12px;
  font-size: 14px;
  line-height: 1.6;
}

.review-issues {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.review-issue {
  padding: 8px;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  flex-wrap: wrap;
}

.issue-file {
  font-family: monospace;
  font-size: 12px;
  color: #606266;
}

.issue-desc {
  width: 100%;
  font-size: 13px;
  color: #303133;
  margin-top: 4px;
}

.commit-scope {
  margin-top: 12px;
  max-height: 180px;
  overflow-y: auto;
  border-top: 1px solid #f0f0f0;
  padding-top: 8px;
}

.scope-item {
  display: flex;
  gap: 4px;
  font-size: 13px;
  padding: 2px 0;
}

.scope-repo {
  font-weight: 500;
}

.scope-count {
  color: #909399;
}

.scope-skipped {
  font-size: 12px;
  color: #909399;
  margin-top: 8px;
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

.push-repo-list {
  margin: 12px 0 0;
  padding: 0;
  list-style: none;
  max-height: 260px;
  overflow-y: auto;
}

.push-repo-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 0;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}
</style>
