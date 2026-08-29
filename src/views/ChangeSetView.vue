<template>
  <div class="change-set-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button
          type="primary"
          :disabled="!workspaceStore.currentWorkspace"
          @click="openCreateDialog"
        >
          <template #icon><n-icon><AddOutline /></n-icon></template>
          新建 Change Set
        </n-button>
        <n-button
          :disabled="!workspaceStore.currentWorkspace"
          :loading="store.loading"
          @click="reloadList"
        >
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
      <div class="toolbar-right">
      </div>
    </div>

    <div class="main-body">
      <!-- Left: change set list -->
      <!-- F-18：n-spin 渲染为 .n-spin-container，是 .main-body 的直接 flex
           子项，必须显式参与布局（同 F-09b），否则内部高度塌陷、空状态挤左上角 -->
      <n-spin :show="store.loading" class="list-spin">
        <div class="set-list">
          <div
            v-for="cs in store.changeSets"
            :key="cs.id"
            :class="['set-item', { active: cs.id === store.currentId }]"
            @click="store.selectChangeSet(cs.id)"
          >
            <div class="set-item-title">
              <span class="set-name">{{ cs.name }}</span>
              <n-button
                size="small"
                quaternary
                type="error"
                @click.stop="handleDeleteSet(cs)"
              >
                <template #icon><n-icon><TrashOutline /></n-icon></template>
              </n-button>
            </div>
            <div v-if="cs.description" class="set-desc">{{ cs.description }}</div>
            <div class="set-updated">更新于 {{ formatTime(cs.updatedAt) }}</div>
          </div>
          <n-empty
            v-if="!store.loading && store.changeSets.length === 0"
            description="还没有 Change Set"
          >
            <n-button
              type="primary"
              size="small"
              :disabled="!workspaceStore.currentWorkspace"
              @click="openCreateDialog"
            >
              新建 Change Set
            </n-button>
          </n-empty>
        </div>
      </n-spin>

      <!-- Right: selected change set detail -->
      <n-spin :show="store.summaryLoading" class="detail-spin">
        <div class="set-detail">
          <template v-if="summary">
            <div class="detail-header">
              <div class="detail-title">
                <span class="detail-name">{{ summary.changeSet.name }}</span>
                <n-button size="small" quaternary @click="openEditDialog">
                  <template #icon><n-icon><CreateOutline /></n-icon></template>
                </n-button>
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
              <n-button size="small" type="primary" dashed @click="openAddDialog">
                <template #icon><n-icon><LinkOutline /></n-icon></template>
                添加仓库
              </n-button>
              <n-button
                size="small"
                :disabled="summary.repositories === 0"
                @click="openDiffDialog"
              >
                <template #icon><n-icon><EyeOutline /></n-icon></template>
                View All Diff
              </n-button>
              <n-button
                size="small"
                :disabled="summary.repositories === 0"
                @click="aiPicker.show = true"
              >
                <template #icon><n-icon><SparklesOutline /></n-icon></template>
                AI Review
              </n-button>
              <n-button
                size="small"
                type="primary"
                :disabled="summary.repositories === 0"
                :loading="commitDialog.collecting"
                @click="openCommitDialog"
              >
                <template #icon><n-icon><CreateOutline /></n-icon></template>
                Commit All
              </n-button>
              <n-button
                size="small"
                type="warning"
                dashed
                :disabled="summary.repositories === 0"
                @click="pushDialog.show = true"
              >
                <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
                Push All
              </n-button>
              <n-tooltip trigger="hover">
                <template #trigger>
                  <span>
                    <n-button size="small" disabled>
                      <template #icon><n-icon><RocketOutline /></n-icon></template>
                      Create PRs（T-29）
                    </n-button>
                  </span>
                </template>
                Create PRs 由 T-29 实现（尚未开发）
              </n-tooltip>
              <n-button size="small" quaternary @click="store.refreshSummary()">
                <template #icon><n-icon><RefreshOutline /></n-icon></template>
                刷新统计
              </n-button>
            </div>

            <!-- Member repo table -->
            <n-data-table
              :columns="repoColumns"
              :data="summary.repos"
              size="small"
              class="repo-table"
            />
          </template>
          <n-empty v-else description="选择左侧的 Change Set，或新建一个" />
        </div>
      </n-spin>
    </div>

    <!-- Create / edit change set dialog -->
    <n-modal
      v-model:show="setDialog.show"
      preset="card"
      :title="setDialog.editingId == null ? '新建 Change Set' : '编辑 Change Set'"
      style="width: 480px"
    >
      <n-form label-width="70px">
        <n-form-item label="名称" required>
          <n-input
            v-model:value="setDialog.name"
            placeholder="如：Feature: AI Review"
            maxlength="80"
          />
        </n-form-item>
        <n-form-item label="描述">
          <n-input v-model:value="setDialog.description" type="textarea" :rows="3" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="setDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="setDialog.saving" @click="saveSet">
          {{ setDialog.editingId == null ? "创建" : "保存" }}
        </n-button>
      </template>
    </n-modal>

    <!-- Add repositories dialog (T-20 selector 联动) -->
    <n-modal v-model:show="addDialog.show" preset="card" title="添加仓库到 Change Set" style="width: 780px">
      <div class="selector-row">
        <n-input
          v-model:value="addDialog.selector"
          size="small"
          placeholder="选择器：@group:frontend @tag:p0 @status:dirty 或名称关键字（T-20）"
          clearable
          style="flex: 1"
          @keyup.enter="applySelector"
        />
        <n-button
          size="small"
          :loading="addDialog.selectorLoading"
          :disabled="!addDialog.selector.trim()"
          @click="applySelector"
        >
          <template #icon><n-icon><CheckmarkCircleOutline /></n-icon></template>
          按选择器勾选
        </n-button>
      </div>
      <n-data-table
        ref="addTableRef"
        :columns="addColumns"
        :data="addDialog.repos"
        size="small"
        :max-height="360"
        :row-class-name="addRowClassName"
        :loading="addDialog.loading"
        :checked-row-keys="addCheckedKeys"
        @update:checked-row-keys="onAddSelectionChange"
      />
      <template #footer>
        <n-button @click="addDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="addDialog.saving"
          :disabled="addDialog.checked.length === 0"
          @click="saveAddRepos"
        >
          添加（{{ addDialog.checked.length }}）
        </n-button>
      </template>
    </n-modal>

    <!-- Target branch edit dialog -->
    <n-modal v-model:show="branchDialog.show" preset="card" title="设置目标分支" style="width: 420px">
      <div class="branch-dialog-body">
        <span class="branch-dialog-repo">{{ branchDialog.repoName }}</span>
        <n-input
          v-model:value="branchDialog.branch"
          placeholder="留空 = 清除目标分支"
          clearable
        />
      </div>
      <template #footer>
        <n-button @click="branchDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="branchDialog.saving" @click="saveBranch">
          保存
        </n-button>
      </template>
    </n-modal>

    <!-- View All Diff dialog（多仓库聚合，按仓库懒加载） -->
    <n-modal
      v-model:show="diffDialog.show"
      preset="card"
      :title="`View All Diff — ${summary?.changeSet.name ?? ''}`"
      style="width: 92%"
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
            <n-tag size="small" :bordered="false">{{ row.files }} 文件</n-tag>
          </div>
        </div>
        <n-spin :show="diffDialog.loading">
          <div class="all-diff-files">
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
            <n-empty
              v-if="!diffDialog.loading && diffDialog.repoPath && diffDialog.files.length === 0"
              description="该仓库没有变更"
            />
            <n-empty
              v-else-if="!diffDialog.repoPath"
              description="选择左侧仓库查看 Diff"
            />
          </div>
        </n-spin>
        <div class="all-diff-content">
          <UnifiedDiff v-if="diffDialog.file" :file="diffDialog.file" />
          <n-empty v-else description="选择文件查看 Diff" />
        </div>
      </div>
    </n-modal>

    <!-- AI Review: repo picker -->
    <n-modal v-model:show="aiPicker.show" preset="card" title="AI Review — 选择仓库" style="width: 420px">
      <n-radio-group v-model:value="aiPicker.repoPath" class="ai-repo-list">
        <n-radio
          v-for="row in summary?.repos ?? []"
          :key="row.repo.repoPath"
          :value="row.repo.repoPath"
        >
          {{ row.repo.repoName }}（{{ row.files }} 个文件变更）
        </n-radio>
      </n-radio-group>
      <template #footer>
        <n-button @click="aiPicker.show = false">取消</n-button>
        <n-button
          type="primary"
          :disabled="!aiPicker.repoPath"
          :loading="aiPicker.loading"
          @click="startAiReview"
        >
          开始审查
        </n-button>
      </template>
    </n-modal>

    <!-- AI Review result -->
    <n-modal
      v-model:show="aiPicker.showResult"
      preset="card"
      title="AI Code Review"
      style="width: 600px"
      :close-on-esc="false"
      :mask-closable="false"
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
            <n-tag
              :type="issue.severity === 'high' ? 'error' : issue.severity === 'medium' ? 'warning' : 'info'"
              size="small"
            >
              {{ issue.severity }}
            </n-tag>
            <n-tag size="small" :bordered="false">{{ issue.category }}</n-tag>
            <span class="issue-file">{{ issue.file }}</span>
            <div class="issue-desc">{{ issue.description }}</div>
          </div>
        </div>
        <n-empty v-else description="No issues found" />
      </div>
    </n-modal>

    <!-- Commit All dialog -->
    <n-modal v-model:show="commitDialog.show" preset="card" title="Commit All（批量提交）" style="width: 560px">
      <n-input
        v-model:value="commitDialog.message"
        type="textarea"
        :rows="3"
        placeholder="请输入 commit message（作用于以下所有仓库）"
      />
      <n-checkbox v-model:checked="commitDialog.thenPush" size="small" style="margin-top: 8px">
        提交后 Push
      </n-checkbox>
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
        <n-button @click="commitDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="commitDialog.submitting"
          :disabled="commitDialog.scopes.length === 0"
          @click="handleCommitAll"
        >
          提交（{{ commitDialog.scopes.length }} 个仓库）
        </n-button>
      </template>
    </n-modal>

    <!-- Pre-commit safety findings dialog (T-11 §5) -->
    <n-modal v-model:show="scanDialog.show" preset="card" title="提交安全检查" style="width: 560px">
      <n-alert type="warning" :bordered="false">
        发现以下风险项，确认无误后可放行提交：
      </n-alert>
      <ul class="scan-finding-list">
        <li v-for="(f, i) in scanDialog.findings" :key="i">
          <n-tag size="small" :type="f.kind === 'forbidden' ? 'error' : 'warning'">
            {{ f.kind }}
          </n-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <n-button @click="scanDialog.show = false">取消</n-button>
        <n-button type="error" @click="commitAllWithOverride">仍要提交</n-button>
      </template>
    </n-modal>

    <!-- Push All confirm dialog -->
    <n-modal v-model:show="pushDialog.show" preset="card" title="Push All（批量推送）" style="width: 520px">
      <template v-if="pushCandidates.length > 0">
        <n-alert type="info" :bordered="false">
          将推送以下 {{ pushCandidates.length }} 个仓库的当前分支
        </n-alert>
        <ul class="push-repo-list">
          <li v-for="row in pushCandidates" :key="row.repo.repoPath">
            <span class="repo-name">{{ row.repo.repoName }}</span>
            <n-tag size="small" :bordered="false">{{ row.currentBranch }}</n-tag>
            <span class="ahead">↑{{ row.ahead }}</span>
          </li>
        </ul>
        <div v-if="pushSkipped.length > 0" class="scope-skipped">
          无需推送（本地不领先）：{{ pushSkipped.join("、") }}
        </div>
      </template>
      <n-empty v-else description="所有关联仓库均已同步（无待推送提交）" />
      <template #footer>
        <n-button @click="pushDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="pushDialog.loading"
          :disabled="pushCandidates.length === 0"
          @click="doPushAll"
        >
          Push（{{ pushCandidates.length }}）
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, nextTick, onMounted, onUnmounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  AddOutline,
  RefreshOutline,
  EyeOutline,
  SparklesOutline,
  CreateOutline,
  CloudUploadOutline,
  TrashOutline,
  LinkOutline,
  RocketOutline,
  CheckmarkCircleOutline,
} from "@vicons/ionicons5";
import { NButton, NIcon, NTag, type DataTableColumns } from "naive-ui";
import { useMessage, useDialog } from "naive-ui";
import { useRouter } from "vue-router";
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

const message = useMessage();
const dialog = useDialog();
const router = useRouter();

const workspaceStore = useWorkspaceStore();
const taskStore = useTaskStore();
const store = useChangeSetStore();

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

const addCheckedKeys = computed(() =>
  addDialog.value.checked
    .map((r) => r.repository.id)
    .filter((id): id is number => id != null),
);

function addRowClassName(row: RepositoryWithStatus) {
  return isMember(row) ? "row-disabled" : "";
}

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

// --- n-data-table columns for repo table ---
const repoColumns: DataTableColumns<ChangeSetRepoSummary> = [
  {
    title: "仓库",
    key: "repo",
    minWidth: 160,
    render(row) {
      return h("div", { class: "repo-cell" }, [
        h("span", { class: "repo-name" }, row.repo.repoName),
        h("span", { class: "repo-rel" }, row.repo.relativePath),
      ]);
    },
  },
  {
    title: "当前分支",
    key: "currentBranch",
    width: 130,
    render(row) {
      return h(NTag, { size: "small", bordered: false }, { default: () => row.currentBranch ?? "—" });
    },
  },
  {
    title: "目标分支",
    key: "targetBranch",
    width: 170,
    render(row) {
      return h("span", {}, [
        h("span", { class: "target-branch" }, row.repo.targetBranch ?? "—"),
        h(
          NButton,
          { size: "small", quaternary: true, onClick: () => openBranchDialog(row) },
          { icon: () => h(NIcon, null, { default: () => h(CreateOutline) }) },
        ),
      ]);
    },
  },
  {
    title: "前/后",
    key: "aheadBehind",
    width: 80,
    align: "center",
    render(row) {
      const parts = [];
      if (row.ahead > 0) parts.push(h("span", { class: "ahead" }, `↑${row.ahead}`));
      if (row.behind > 0) parts.push(h("span", { class: "behind" }, `↓${row.behind}`));
      if (row.ahead === 0 && row.behind === 0) parts.push(h("span", {}, "—"));
      return h("span", {}, parts);
    },
  },
  { title: "Files", key: "files", width: 70, align: "center" },
  {
    title: "+/-",
    key: "changes",
    width: 110,
    align: "center",
    render(row) {
      return h("span", {}, [
        h("span", { class: "added" }, `+${row.added}`),
        h("span", { class: "deleted" }, ` -${row.deleted}`),
      ]);
    },
  },
  {
    title: "状态",
    key: "status",
    minWidth: 120,
    render(row) {
      if (row.error) {
        return h(NTag, { size: "small", type: "error" }, { default: () => row.error });
      } else if (row.files > 0) {
        return h(NTag, { size: "small", type: "warning" }, { default: () => "有变更" });
      } else {
        return h(NTag, { size: "small", type: "success" }, { default: () => "干净" });
      }
    },
  },
  {
    key: "actions",
    width: 70,
    align: "center",
    render(row) {
      return h(
        NButton,
        {
          size: "small",
          quaternary: true,
          type: "error",
          onClick: () => handleRemoveRepo(row),
        },
        { icon: () => h(NIcon, null, { default: () => h(TrashOutline) }) },
      );
    },
  },
];

// --- n-data-table columns for add-repo table ---
const addColumns: DataTableColumns<RepositoryWithStatus> = [
  { type: "selection", disabled(row: RepositoryWithStatus) { return isMember(row); } },
  {
    title: "仓库",
    key: "repo",
    minWidth: 170,
    render(row) {
      const children = [
        h("div", { class: "repo-cell" }, [
          h("span", { class: "repo-name" }, row.repository.name),
          h("span", { class: "repo-rel" }, row.repository.relativePath),
        ]),
      ];
      if (isMember(row)) {
        children.push(h(NTag, { size: "small", type: "info", bordered: false }, { default: () => "已关联" }));
      }
      return h("span", {}, children);
    },
  },
  {
    title: "当前分支",
    key: "branch",
    width: 120,
    render(row) {
      return h(NTag, { size: "small", bordered: false }, { default: () => row.status?.branch ?? "—" });
    },
  },
  {
    title: "目标分支（默认当前分支）",
    key: "targetBranch",
    minWidth: 180,
    render(row) {
      return h(NInput, {
        value: addDialog.value.branches[row.repository.id ?? -1],
        "onUpdate:value": (val: string) => {
          addDialog.value.branches[row.repository.id ?? -1] = val;
        },
        size: "small",
        placeholder: "feature/xxx",
        disabled: isMember(row),
      });
    },
  },
];

// Need NInput for the addColumns render function
import { NInput } from "naive-ui";

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
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
      if (workspaceStore.currentWorkspace) {
        store.loadChangeSets(workspaceStore.currentWorkspace.id).catch(() => {});
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

// Helper so onMounted stays linear (load list for the current workspace).
async function loadCurrentList() {
  const wsId = workspaceStore.currentWorkspace?.id;
  if (wsId) {
    await store.loadChangeSets(wsId);
  }
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
    message.warning("请输入名称");
    return;
  }
  d.saving = true;
  try {
    if (d.editingId == null) {
      const cs = await store.createSet({
        workspaceId: workspaceStore.currentWorkspace!.id,
        name,
        description: d.description.trim() || null,
      });
      message.success("已创建 Change Set");
      d.show = false;
      await store.selectChangeSet(cs.id);
    } else {
      await store.updateSet({
        id: d.editingId,
        name,
        description: d.description.trim() || null,
      });
      message.success("已保存");
      d.show = false;
    }
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  } finally {
    d.saving = false;
  }
}

async function handleDeleteSet(cs: ChangeSet) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "删除确认",
        content: `确定删除 Change Set「${cs.name}」吗？仅删除关联关系，不影响任何仓库代码。`,
        positiveText: "删除",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject(new Error("cancel")),
        onClose: () => reject(new Error("cancel")),
      });
    });
  } catch {
    return;
  }
  try {
    await store.removeSet(cs.id);
    message.success("已删除");
  } catch (e) {
    message.error("删除失败: " + errMsg(e));
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
  const wsId = workspaceStore.currentWorkspace?.id;
  if (!wsId) return;
  addDialog.value.show = true;
  addDialog.value.loading = true;
  addDialog.value.selector = "";
  addDialog.value.checked = [];
  try {
    const repos = await listRepositories(wsId);
    addDialog.value.repos = repos;
    const branches: Record<number, string> = {};
    for (const r of repos) {
      const id = r.repository.id;
      if (id != null) branches[id] = r.status?.branch ?? "";
    }
    addDialog.value.branches = branches;
    await nextTick();
  } catch (e) {
    message.error("加载仓库列表失败: " + errMsg(e));
  } finally {
    addDialog.value.loading = false;
  }
}

function onAddSelectionChange(keys: Array<number | string>) {
  const keySet = new Set(keys.map(Number));
  addDialog.value.checked = addDialog.value.repos.filter(
    (r) => r.repository.id != null && keySet.has(r.repository.id),
  );
}

/** T-20 联动：用选择器结果勾选仓库。 */
async function applySelector() {
  const query = addDialog.value.selector.trim();
  const wsId = workspaceStore.currentWorkspace?.id;
  if (!query || !wsId) return;
  addDialog.value.selectorLoading = true;
  try {
    const paths = await selectRepos(wsId, query);
    const matched = new Set(paths);
    // Select matching repos that are not already members
    addDialog.value.checked = addDialog.value.repos.filter(
      (r) => matched.has(r.repository.path) && !isMember(r),
    );
    message.success(`选择器命中 ${paths.length} 个仓库，已勾选可添加项`);
  } catch (e) {
    message.error("选择器查询失败: " + errMsg(e));
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
    message.success(`已关联 ${inputs.length} 个仓库`);
    addDialog.value.show = false;
  } catch (e) {
    message.error("添加失败: " + errMsg(e));
  } finally {
    addDialog.value.saving = false;
  }
}

async function handleRemoveRepo(row: ChangeSetRepoSummary) {
  const csId = store.currentId;
  if (csId == null) return;
  try {
    await store.removeRepo(csId, row.repo.repoId);
    message.success(`已移除 ${row.repo.repoName}`);
  } catch (e) {
    message.error("移除失败: " + errMsg(e));
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
    message.success("已更新目标分支");
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
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
    message.error("获取 Diff 失败: " + errMsg(e));
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
  // AI-01：Provider/模型/凭证由 AI 设置解析，不再弹窗索要 Key（§12.4）。
  aiPicker.value.loading = true;
  try {
    aiPicker.value.result = await aiReview(repoPath);
    aiPicker.value.show = false;
    aiPicker.value.showResult = true;
  } catch (e) {
    const code = (e as { code?: string })?.code;
    if (code === "AiNotConfigured" || code === "AiCredentialUnavailable") {
      message.error(errMsg(e));
      router.push({ name: "ai-settings" });
    } else {
      message.error("AI Review 失败: " + errMsg(e));
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
      message.info("所有关联仓库都没有可提交的变更");
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
  const msg = commitDialog.value.message.trim();
  if (!msg) {
    message.warning("请输入提交信息");
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
    message.error("安全检查失败: " + errMsg(e));
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
  const msg = commitDialog.value.message.trim();
  const commits: CommitRequest[] = commitDialog.value.scopes.map((scope) => ({
    repoPath: scope.repoPath,
    repoName: scope.repoName,
    message: msg,
    files: scope.files,
    thenPush: commitDialog.value.thenPush,
    allowUnsafe,
  }));
  commitDialog.value.submitting = true;
  try {
    const taskIds = await batchCommit(commits);
    message.success(`已提交 ${taskIds.length} 个 commit 任务，进度见任务面板`);
    commitDialog.value.show = false;
    taskStore.showPanel();
  } catch (e) {
    message.error("提交失败: " + errMsg(e));
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
    message.success(`已提交 ${taskIds.length} 个 push 任务，进度见任务面板`);
    pushDialog.value.show = false;
    taskStore.showPanel();
  } catch (e) {
    message.error("push 失败: " + errMsg(e));
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
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.main-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

/* F-18：两个 n-spin 容器作为 .main-body 的 flex 子项参与布局，
   高度链经 .n-spin-content 打通到内部面板。 */
.list-spin {
  width: 280px;
  flex-shrink: 0;
  min-height: 0;
}

.detail-spin {
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.list-spin :deep(.n-spin-content),
.detail-spin :deep(.n-spin-content) {
  height: 100%;
}

.set-list {
  height: 100%;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
  background: var(--gw-bg-hover);
  padding: 8px;
  display: flex;
  flex-direction: column;
}

/* F-18：空状态时 n-empty 在列表区居中（有数据时不影响列表项流式排列） */
.set-list > .n-empty {
  flex: 1;
  justify-content: center;
}

.set-item {
  padding: 10px 12px;
  border-radius: 6px;
  cursor: pointer;
  border: 1px solid transparent;
  margin-bottom: 6px;
  background: var(--gw-bg-panel);
}

.set-item:hover {
  border-color: var(--gw-accent);
}

.set-item.active {
  border-color: var(--gw-accent);
  background: var(--gw-bg-hover);
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
  color: var(--gw-text-dim);
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.set-updated {
  font-size: 11px;
  color: var(--gw-text-dim);
  margin-top: 4px;
}

.set-detail {
  height: 100%;
  overflow-y: auto;
  padding: 16px;
}

/* F-18：未选中 Change Set 时，右侧空状态在详情区居中 */
.set-detail > .n-empty {
  height: 100%;
  justify-content: center;
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
  color: var(--gw-text-dim);
  margin-top: 4px;
}

.stats-cards {
  display: flex;
  gap: var(--gw-space-3);
  margin-bottom: 12px;
}

.stat-card {
  flex: 1;
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
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
  color: var(--gw-success);
}

.stat-value.deleted,
.deleted {
  color: var(--gw-danger);
}

.stat-label {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin-top: 2px;
}

.action-bar {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
  flex-wrap: wrap;
  margin-bottom: 12px;
}

.action-bar .n-button + .n-button {
  margin-left: 0;
}

.repo-table {
  border: 1px solid var(--gw-border);
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
  color: var(--gw-text-dim);
}

.target-branch {
  margin-right: 4px;
  font-family: monospace;
  font-size: 12px;
}

.ahead {
  color: var(--gw-warning);
  margin-right: 4px;
}

.behind {
  color: var(--gw-accent);
}

.selector-row {
  display: flex;
  gap: var(--gw-space-2);
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
  border: 1px solid var(--gw-border);
  border-radius: 6px;
  overflow: hidden;
}

.all-diff-repos {
  width: 200px;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
  background: var(--gw-bg-hover);
}

.all-diff-repo {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 10px;
  cursor: pointer;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.all-diff-repo:hover {
  background: var(--gw-bg-hover);
}

.all-diff-repo.active {
  background: var(--gw-bg-hover);
  border-left: 3px solid var(--gw-accent);
  padding-left: 7px;
}

.all-diff-files {
  width: 260px;
  border-right: 1px solid var(--gw-border);
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
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.file-item:hover {
  background: var(--gw-bg-hover);
}

.file-item.active {
  background: var(--gw-bg-hover);
  border-left: 3px solid var(--gw-accent);
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
  color: var(--gw-success);
}

.file-status-icon.deleted {
  color: var(--gw-danger);
}

.file-status-icon.modified {
  color: var(--gw-warning);
}

.file-status-icon.renamed {
  color: var(--gw-text-dim);
}

.file-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ai-repo-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
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
  gap: var(--gw-space-2);
}

.review-issue {
  padding: 8px;
  border: 1px solid var(--gw-border);
  border-radius: 4px;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  flex-wrap: wrap;
}

.issue-file {
  font-family: monospace;
  font-size: 12px;
  color: var(--gw-text-dim);
}

.issue-desc {
  width: 100%;
  font-size: 13px;
  color: var(--gw-text);
  margin-top: 4px;
}

.commit-scope {
  margin-top: 12px;
  max-height: 180px;
  overflow-y: auto;
  border-top: 1px solid var(--gw-border);
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
  color: var(--gw-text-dim);
}

.scope-skipped {
  font-size: 12px;
  color: var(--gw-text-dim);
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
  gap: var(--gw-space-2);
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.scan-path {
  font-family: monospace;
  color: var(--gw-text);
}

.scan-detail {
  color: var(--gw-text-dim);
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
  gap: var(--gw-space-2);
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}
</style>
