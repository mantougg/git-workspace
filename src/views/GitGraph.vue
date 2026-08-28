<template>
  <div class="git-graph-view">
    <!-- Header -->
    <div class="graph-header">
      <RepoSwitcher @change="onRepoSwitch" />
      <n-button
        type="primary"
        size="small"
        :loading="loading"
        @click="loadHistory"
      >
        刷新
      </n-button>
    </div>

    <!-- Branch bar -->
    <div v-if="branches.length > 0" class="branch-bar">
      <n-tag
        v-for="branch in branches.slice(0, 10)"
        :key="branch.name"
        :type="branch.isCurrent ? 'success' : branch.isRemote ? 'warning' : 'default'"
        size="small"
        :bordered="false"
      >
        {{ branch.name }}
      </n-tag>
    </div>

    <!-- In-progress conflict banner (T-13; the T-16 resolver hooks in here) -->
    <div v-if="conflictFiles.length > 0" class="conflict-bar">
      <span class="conflict-text">
        存在未解决的冲突（{{ conflictFiles.length }} 个文件）：{{ conflictFiles.join("、") }}
      </span>
      <n-button size="small" type="primary" dashed @click="openResolver">
        进入解决器
      </n-button>
      <n-button size="small" type="error" dashed @click="abortInProgress()">
        中止并恢复（Abort）
      </n-button>
      <span class="conflict-hint">可手动编辑解决后提交；三方解决器随 T-16 提供</span>
    </div>

    <!-- Commit graph -->
    <n-spin :show="loading">
      <div class="graph-body">
        <CommitGraph
          :commits="commits"
          :loading="loading"
          :has-more="hasMore"
          @select="onCommitSelect"
          @action="onCommitAction"
          @contextmenu="onCommitContextmenu"
          @load-more="loadMore"
        />
      </div>
    </n-spin>

    <!-- D-13：提交节点右键菜单 -->
    <ContextMenu
      :show="commitMenu.show"
      :options="commitMenuOptions"
      :x="commitMenu.x"
      :y="commitMenu.y"
      @select="onCommitMenuSelect"
      @close="commitMenu.show = false"
    />

    <!-- Reset dialog (T-13) -->
    <n-modal v-model:show="resetDialog.show" preset="card" title="Reset 到此处" style="width: 520px">
      <div v-if="resetDialog.commit" class="reset-target">
        目标提交：{{ resetDialog.commit.shortOid }} {{ firstLine(resetDialog.commit.message) }}
      </div>
      <n-radio-group v-model:value="resetDialog.mode" class="reset-modes">
        <n-radio value="soft">soft — 仅移动 HEAD，保留暂存区与工作区</n-radio>
        <n-radio value="mixed">mixed — 移动 HEAD + 重置暂存区，保留工作区</n-radio>
        <n-radio value="hard">hard — 重置全部，丢弃未提交更改（危险）</n-radio>
      </n-radio-group>
      <template #footer>
        <n-button @click="resetDialog.show = false">取消</n-button>
        <n-button
          :type="resetDialog.mode === 'hard' ? 'error' : 'primary'"
          @click="confirmReset"
        >
          执行 Reset
        </n-button>
      </template>
    </n-modal>

    <!-- Conflict outcome dialog (T-13) -->
    <n-modal v-model:show="conflictDialog.show" preset="card" title="操作冲突" style="width: 560px">
      <div class="conflict-dialog-body">
        <p>
          {{ conflictDialog.opLabel }}在应用
          <code>{{ conflictDialog.current.slice(0, 7) }}</code>
          时发生冲突（已完成 {{ conflictDialog.done }}/{{ conflictDialog.total }}）。
        </p>
        <p>冲突文件：</p>
        <ul class="conflict-file-list">
          <li v-for="f in conflictDialog.files" :key="f">{{ f }}</li>
        </ul>
        <p class="conflict-note">
          仓库当前保持冲突状态：可关闭后手动编辑解决（三方解决器随 T-16 提供），或立即中止恢复到操作前状态。
        </p>
      </div>
      <template #footer>
        <n-button @click="conflictDialog.show = false">稍后手动解决</n-button>
        <n-button type="error" @click="abortFromDialog">中止并恢复（Abort）</n-button>
      </template>
    </n-modal>

    <!-- Commit detail -->
    <n-drawer
      v-model:show="showDetail"
      title="提交详情"
      placement="right"
      width="400px"
    >
      <n-drawer-content>
        <div v-if="selectedCommit" class="commit-detail">
          <n-descriptions :column="1" bordered label-placement="left">
            <n-descriptions-item label="Hash">
              {{ selectedCommit.oid }}
            </n-descriptions-item>
            <n-descriptions-item label="作者">
              {{ selectedCommit.author }}
              &lt;{{ selectedCommit.email }}&gt;
            </n-descriptions-item>
            <n-descriptions-item label="时间">
              {{ selectedCommit.time }}
            </n-descriptions-item>
            <n-descriptions-item label="Refs">
              <n-tag
                v-for="ref in selectedCommit.refs"
                :key="ref"
                size="small"
                style="margin-right: 4px"
              >
                {{ ref }}
              </n-tag>
            </n-descriptions-item>
            <n-descriptions-item label="提交信息">
              <pre class="commit-message-full">{{ selectedCommit.message }}</pre>
            </n-descriptions-item>
          </n-descriptions>
          <n-button
            type="primary"
            dashed
            style="margin-top: 12px"
            @click="viewCommitDiff"
          >
            查看 Diff
          </n-button>
        </div>
      </n-drawer-content>
    </n-drawer>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { useMessage, useDialog } from "naive-ui";
import { getCommitHistory, getBranches } from "@/api/graph";
import {
  abortPick,
  cherryPick,
  getConflictFiles,
  resetTo,
  revertCommit,
} from "@/api/history";
import type { PickOutcome } from "@/types/history";
import type { CommitInfo, BranchInfo } from "@/types/graph";
import CommitGraph from "@/components/graph/CommitGraph.vue";
import ContextMenu from "@/components/shell/ContextMenu.vue";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { resolveCurrentRepo } = useCurrentRepo();
const dialog = useDialog();

const repoPath = ref("");
const commits = ref<CommitInfo[]>([]);
const branches = ref<BranchInfo[]>([]);
const loading = ref(false);
const hasMore = ref(false);
const showDetail = ref(false);
const selectedCommit = ref<CommitInfo | null>(null);

// --- T-13 history operations state ---
const conflictFiles = ref<string[]>([]);
const resetDialog = reactive<{
  show: boolean;
  commit: CommitInfo | null;
  mode: "soft" | "mixed" | "hard";
}>({ show: false, commit: null, mode: "mixed" });
const conflictDialog = reactive<{
  show: boolean;
  opLabel: string;
  files: string[];
  current: string;
  done: number;
  total: number;
  baseOid: string | null;
}>({ show: false, opLabel: "", files: [], current: "", done: 0, total: 0, baseOid: null });

const PAGE_SIZE = 100;

onMounted(async () => {
  // F-14/F-17：query → 全局当前仓库 → 工作区首仓库兜底（SideNav 直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await loadHistory();
  await loadBranches();
  await refreshConflicts();
});

async function loadHistory() {
  loading.value = true;
  try {
    commits.value = await getCommitHistory(repoPath.value, PAGE_SIZE);
    hasMore.value = commits.value.length >= PAGE_SIZE;
  } catch (e) {
    message.error("加载提交历史失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function loadBranches() {
  try {
    branches.value = await getBranches(repoPath.value);
  } catch (e) {
    console.error("Failed to load branches:", e);
  }
}

async function loadMore() {
  loading.value = true;
  try {
    const more = await getCommitHistory(
      repoPath.value,
      commits.value.length + PAGE_SIZE,
    );
    if (more.length > commits.value.length) {
      commits.value = more;
      hasMore.value = more.length >= commits.value.length + PAGE_SIZE;
    } else {
      hasMore.value = false;
    }
  } catch (e) {
    message.error("加载更多失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function onCommitSelect(commit: CommitInfo) {
  selectedCommit.value = commit;
  showDetail.value = true;
}

// --- T-13 history operations ---

function firstLine(message: string): string {
  return message.split("\n")[0];
}

function currentBranchName(): string {
  return branches.value.find((b) => b.isCurrent)?.name ?? "HEAD";
}

async function refreshConflicts() {
  try {
    conflictFiles.value = await getConflictFiles(repoPath.value);
  } catch {
    conflictFiles.value = [];
  }
}

// F-22：切换仓库后重置视图状态并重载（切换时不会重新挂载，
// 不能再走 resolveCurrentRepo，显式重调各加载入口）。
async function onRepoSwitch(path: string) {
  repoPath.value = path;
  commits.value = [];
  branches.value = [];
  conflictFiles.value = [];
  hasMore.value = false;
  showDetail.value = false;
  selectedCommit.value = null;
  await loadHistory();
  await loadBranches();
  await refreshConflicts();
}

function onCommitAction(action: string, commit: CommitInfo) {
  switch (action) {
    case "cherry-pick":
      handleCherryPick(commit);
      break;
    case "revert":
      handleRevert(commit);
      break;
    case "reset":
      resetDialog.commit = commit;
      resetDialog.mode = "mixed";
      resetDialog.show = true;
      break;
  }
}

// --- D-13：提交节点右键菜单（复用 T-13 历史操作与确认流） ---
const commitMenu = ref({
  show: false,
  x: 0,
  y: 0,
  commit: null as CommitInfo | null,
});

const commitMenuOptions = [
  { label: "Cherry-pick", key: "cherry-pick" },
  { label: "Revert", key: "revert" },
  { type: "divider", key: "d1" },
  { label: "Reset 到此处…", key: "reset" },
  {
    label: "Reset --hard 到此处",
    key: "reset-hard",
    props: { style: "color: var(--gw-danger)" },
  },
  { type: "divider", key: "d2" },
  { label: "Copy hash", key: "copy-hash" },
  { label: "查看 Diff", key: "diff" },
];

function onCommitContextmenu(commit: CommitInfo, x: number, y: number) {
  commitMenu.value = { show: true, x, y, commit };
}

async function onCommitMenuSelect(key: string) {
  const commit = commitMenu.value.commit;
  if (!commit) return;
  switch (key) {
    case "cherry-pick":
    case "revert":
    case "reset":
      onCommitAction(key, commit);
      break;
    case "reset-hard":
      resetDialog.commit = commit;
      resetDialog.mode = "hard";
      resetDialog.show = true;
      break;
    case "copy-hash":
      try {
        await navigator.clipboard.writeText(commit.oid);
        message.success(`已复制 ${commit.shortOid}`);
      } catch {
        message.error("复制失败");
      }
      break;
    case "diff":
      router.push({
        name: "diff-viewer",
        query: { repo: repoPath.value, commit: commit.oid },
      });
      break;
  }
}

/** Warning-level confirm (§46) for history-modifying ops. */
async function confirmOp(title: string, detail: string): Promise<boolean> {
  return new Promise((resolve) => {
    dialog.warning({
      title,
      content: detail,
      positiveText: "执行",
      negativeText: "取消",
      onPositiveClick: () => resolve(true),
      onNegativeClick: () => resolve(false),
      onClose: () => resolve(false),
    });
  });
}

function handleOutcome(outcome: PickOutcome, opLabel: string) {
  if (outcome.status === "success") {
    message.success(`${opLabel}完成（${outcome.picked} 个提交）`);
  } else {
    conflictDialog.opLabel = opLabel;
    conflictDialog.files = outcome.files;
    conflictDialog.current = outcome.current;
    conflictDialog.done = outcome.done;
    conflictDialog.total = outcome.total;
    conflictDialog.baseOid = outcome.baseOid;
    conflictDialog.show = true;
  }
}

async function afterHistoryOp() {
  await loadHistory();
  await refreshConflicts();
}

async function handleCherryPick(commit: CommitInfo) {
  const ok = await confirmOp(
    "Cherry-pick 确认",
    `仓库：${repoPath.value}\n当前分支：${currentBranchName()}\n将把提交 ${commit.shortOid}（${firstLine(commit.message)}）应用到当前分支。`,
  );
  if (!ok) return;
  try {
    handleOutcome(await cherryPick(repoPath.value, [commit.oid]), "Cherry-pick");
    await afterHistoryOp();
  } catch (e) {
    message.error("Cherry-pick 失败: " + errMsg(e));
  }
}

async function handleRevert(commit: CommitInfo) {
  const ok = await confirmOp(
    "Revert 确认",
    `仓库：${repoPath.value}\n当前分支：${currentBranchName()}\n将回滚提交 ${commit.shortOid}（${firstLine(commit.message)}）并生成 revert 提交。`,
  );
  if (!ok) return;
  try {
    handleOutcome(await revertCommit(repoPath.value, commit.oid), "Revert");
    await afterHistoryOp();
  } catch (e) {
    message.error("Revert 失败: " + errMsg(e));
  }
}

async function confirmReset() {
  const commit = resetDialog.commit;
  if (!commit) return;
  const mode = resetDialog.mode;

  if (mode === "hard") {
    // Dangerous (§46): impact scope + data-loss + recovery hint.
    const confirmed = await new Promise<boolean>((resolve) => {
      dialog.error({
        title: "Reset --hard 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n当前分支：${currentBranchName()}\n目标：${commit.shortOid}（${firstLine(commit.message)}）\n\n影响范围：HEAD、暂存区、工作区全部重置到该提交；未提交的更改将丢失，之后的提交将从分支上移除。\n保底：可先 Stash 保存现场；原 HEAD 位置会在执行结果中给出（可用 reflog 找回）。`,
        positiveText: "确认 Hard Reset",
        negativeText: "取消",
        onPositiveClick: () => resolve(true),
        onNegativeClick: () => resolve(false),
        onClose: () => resolve(false),
      });
    });
    if (!confirmed) return;
  }

  resetDialog.show = false;
  try {
    const result = await resetTo(repoPath.value, commit.oid, mode);
    const prev = result.previousHead ? result.previousHead.slice(0, 7) : "无";
    message.success(`Reset（${mode}）完成；原 HEAD：${prev}（可在 Reflog 视图恢复）`);
    await afterHistoryOp();
  } catch (e) {
    message.error("Reset 失败: " + errMsg(e));
  }
}

async function abortInProgress(baseOid?: string) {
  const confirmed = await new Promise<boolean>((resolve) => {
    dialog.error({
      title: "中止确认（Dangerous）",
      content: `仓库：${repoPath.value}\n将放弃当前冲突状态并恢复到操作前位置（hard reset）。冲突文件中的修改将丢失。`,
      positiveText: "中止并恢复",
      negativeText: "取消",
      onPositiveClick: () => resolve(true),
      onNegativeClick: () => resolve(false),
      onClose: () => resolve(false),
    });
  });
  if (!confirmed) return;
  try {
    await abortPick(repoPath.value, baseOid);
    message.success("已中止并恢复");
    await afterHistoryOp();
  } catch (e) {
    message.error("中止失败: " + errMsg(e));
  }
}

async function abortFromDialog() {
  const base = conflictDialog.baseOid ?? undefined;
  conflictDialog.show = false;
  await abortInProgress(base);
}

// Commit Diff entry (T-12): open the DiffViewer in commit mode.
function viewCommitDiff() {
  if (!selectedCommit.value) return;
  router.push({
    name: "diff-viewer",
    query: { repo: repoPath.value, commit: selectedCommit.value.oid },
  });
}

function openResolver() {
  router.push({ name: "conflict-resolver", query: { repo: repoPath.value } });
}

</script>

<style scoped>
.git-graph-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.graph-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.repo-path {
  flex: 1;
  font-size: 14px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
}

.branch-bar {
  display: flex;
  gap: 4px;
  padding: 4px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-hover);
  flex-wrap: wrap;
}

.graph-body {
  flex: 1;
  overflow: hidden;
}

.commit-detail {
  padding: 12px;
}

.commit-message-full {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: var(--gw-font-mono);
  font-size: 13px;
  margin: 0;
}
</style>

<style scoped>
.conflict-bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  background: var(--gw-danger);
  border-bottom: 1px solid var(--gw-danger);
  font-size: 13px;
}

.conflict-text {
  flex: 1;
  color: var(--gw-danger);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conflict-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.reset-target {
  margin-bottom: 12px;
  font-size: 13px;
  color: var(--gw-text-dim);
}

.reset-modes {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.conflict-dialog-body p {
  margin: 6px 0;
  font-size: 13px;
}

.conflict-file-list {
  margin: 4px 0;
  padding-left: 20px;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-danger);
  max-height: 160px;
  overflow-y: auto;
}

.conflict-note {
  color: var(--gw-text-dim);
}
</style>
