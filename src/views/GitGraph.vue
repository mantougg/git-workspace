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
        v-for="branch in visibleBranches"
        :key="branch.name"
        :type="branchTagType(branch)"
        size="small"
        :bordered="false"
      >
        {{ branch.name }}
      </n-tag>
      <!-- GF-13b：超出前 10 个的分支折叠为 +N（desktop-skin-plan §5.6「前 10
           分支」规范的对齐做法），点击展开列出其余分支，不再静默截断。 -->
      <n-popover
        v-if="hiddenBranches.length > 0"
        trigger="click"
        placement="bottom-start"
        :width="280"
      >
        <template #trigger>
          <n-tag size="small" :bordered="false" type="info" class="branch-more-tag">
            +{{ hiddenBranches.length }}
          </n-tag>
        </template>
        <n-scrollbar style="max-height: 240px">
          <div class="branch-overflow-list">
            <n-tag
              v-for="branch in hiddenBranches"
              :key="branch.name"
              :type="branchTagType(branch)"
              size="small"
              :bordered="false"
            >
              {{ branch.name }}
            </n-tag>
          </div>
        </n-scrollbar>
      </n-popover>
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
    <n-spin :show="loading" class="graph-spin">
      <div class="graph-body">
        <!-- key=repoPath + refreshSeq：切仓库 / 整页重载（刷新、历史操作后）时
             整体重挂载，VirtualList 滚动位置随之复位；loadMore 追加不换 key，
             滚动位置保持不变（配合 CommitGraph 的 resetScrollOnItemsChange=false）。 -->
        <CommitGraph
          :key="repoPath + '#' + refreshSeq"
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
      <!-- GF-17：结构化预演 —— 「操作会发生什么」（Roadmap §46） -->
      <n-spin :show="resetDialog.previewLoading" size="small" class="reset-preview">
        <div v-if="resetDialog.previewError" class="reset-preview-error">
          预演加载失败：{{ resetDialog.previewError }}（仍可执行，执行时会再次校验）
        </div>
        <template v-else-if="resetDialog.preview">
          <div class="reset-preview-line">
            将丢弃
            <strong :class="{ 'danger-text': resetDialog.preview.discardedCount > 0 }">{{
              resetDialog.preview.discardedCount
            }}</strong>
            个提交
            <span
              v-if="resetDialog.preview.discardedCommits.length < resetDialog.preview.discardedCount"
              class="dim-text"
            >
              （仅显示前 {{ resetDialog.preview.discardedCommits.length }} 个）
            </span>
          </div>
          <ul v-if="resetDialog.preview.discardedCommits.length" class="reset-preview-list">
            <li v-for="c in resetDialog.preview.discardedCommits" :key="c.oid">
              <span class="mono">{{ c.shortOid }}</span>
              <span class="reset-preview-msg">{{ c.summary }}</span>
            </li>
          </ul>
          <div class="reset-preview-line">
            将丢弃
            <strong :class="{ 'danger-text': resetDialog.preview.unrecoverable }">{{
              resetDialog.preview.lostChangesCount
            }}</strong>
            个未提交变更
            <span v-if="resetDialog.preview.unrecoverable" class="danger-text">
              （不可恢复，reflog 无法找回）
            </span>
          </div>
          <ul v-if="resetDialog.preview.lostFileChanges.length" class="reset-preview-list">
            <li
              v-for="f in resetDialog.preview.lostFileChanges"
              :key="f.path"
              :class="{ 'danger-text': resetDialog.preview.unrecoverable }"
            >
              <span class="mono">{{ f.path }}</span>
              <span class="dim-text">（{{ f.status }}）</span>
            </li>
          </ul>
          <div v-if="resetDialog.mode === 'soft'" class="dim-text">
            soft：被丢弃提交的内容会进入暂存区，文件变更不丢失。
          </div>
          <div v-else-if="resetDialog.mode === 'mixed'" class="dim-text">
            mixed：暂存区被重置，未提交内容保留在工作区。
          </div>
        </template>
      </n-spin>
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
          <div class="commit-detail-actions">
            <n-button
              type="primary"
              dashed
              @click="viewCommitDiff"
            >
              查看 Diff
            </n-button>
            <n-button
              type="primary"
              @click="openCommitExplanation"
            >
              AI 解释提交
            </n-button>
          </div>
        </div>
      </n-drawer-content>
    </n-drawer>

    <AiGitAssistantDialog
      v-model="gitAssistantVisible"
      :repositories="assistantRepositories"
      initial-scenario="commitExplanation"
      :supplementary="assistantSupplementary"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { useMessage, useDialog } from "naive-ui";
import { getCommitHistory, getBranches } from "@/api/graph";
import { getCommitDiff } from "@/api/git";
import {
  abortPick,
  cherryPick,
  getConflictFiles,
  resetTo,
  revertCommit,
} from "@/api/history";
import { previewReset } from "@/api/preview";
import type { PickOutcome } from "@/types/history";
import type { ResetPreview } from "@/types/preview";
import type { CommitInfo, BranchInfo } from "@/types/graph";
import type { FileDiff } from "@/types/git";
import CommitGraph from "@/components/graph/CommitGraph.vue";
import ContextMenu from "@/components/shell/ContextMenu.vue";
import AiGitAssistantDialog from "@/components/ai/AiGitAssistantDialog.vue";
import type { SupplementaryContext } from "@/types/ai";
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
const gitAssistantVisible = ref(false);
const assistantCommitDiff = ref<FileDiff[]>([]);
const assistantRepositories = computed(() => repoPath.value ? [{
  repoPath: repoPath.value,
  name: repoPath.value.split(/[\\/]/).filter(Boolean).pop() ?? "repository",
  files: [],
}] : []);
const assistantSupplementary = computed<SupplementaryContext[]>(() => {
  const commit = selectedCommit.value;
  if (!commit) return [];
  const contexts: SupplementaryContext[] = [{
    role: "history",
    kind: "repository",
    sourceId: `commit:${commit.oid}`,
    displayName: `提交 ${commit.shortOid}`,
    content: [
      `commit: ${commit.oid}`,
      `author: ${commit.author} <${commit.email}>`,
      `time: ${commit.time}`,
      `parents: ${commit.parents.join(", ") || "(root commit)"}`,
      "message:",
      commit.message,
    ].join("\n"),
  }];
  if (assistantCommitDiff.value.length > 0) {
    contexts.push({
      role: "fullDiff",
      kind: "diff",
      sourceId: `commit:${commit.oid}:diff`,
      displayName: `提交 ${commit.shortOid} 的 Diff`,
      content: JSON.stringify(assistantCommitDiff.value, null, 2),
    });
  }
  return contexts;
});

// --- T-13 history operations state ---
const conflictFiles = ref<string[]>([]);
const resetDialog = reactive<{
  show: boolean;
  commit: CommitInfo | null;
  mode: "soft" | "mixed" | "hard";
  /** GF-17：结构化预演（只读命令 preview_reset 的结果）。 */
  preview: ResetPreview | null;
  previewLoading: boolean;
  previewError: string;
}>({ show: false, commit: null, mode: "mixed", preview: null, previewLoading: false, previewError: "" });
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

/** desktop-skin-plan §5.6：分支条只展示前 10 个分支，其余折叠为 +N
 *  （GF-13b：点击展开列出，与规范对齐且消除静默截断）。 */
const BRANCH_BAR_VISIBLE = 10;
const visibleBranches = computed(() => branches.value.slice(0, BRANCH_BAR_VISIBLE));
const hiddenBranches = computed(() => branches.value.slice(BRANCH_BAR_VISIBLE));

/** 分支 tag 配色：当前分支 success / 远程 warning / 其余 default。 */
function branchTagType(branch: BranchInfo): "success" | "warning" | "default" {
  return branch.isCurrent ? "success" : branch.isRemote ? "warning" : "default";
}

/** 整页重载计数：仅 loadHistory（刷新 / 切仓库 / 历史操作后）递增，
 *  作为 CommitGraph 的 remount key 之一，复位虚拟列表滚动位置。 */
const refreshSeq = ref(0);

async function openCommitExplanation() {
  const commit = selectedCommit.value;
  if (!commit || !repoPath.value) return;
  try {
    assistantCommitDiff.value = await getCommitDiff(repoPath.value, commit.oid);
    gitAssistantVisible.value = true;
  } catch (error) {
    message.error("加载提交 Diff 失败: " + errMsg(error));
  }
}

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
  // 整页重载 → 递增 refreshSeq 让 CommitGraph 重挂载（滚动复位到顶）。
  refreshSeq.value += 1;
  loading.value = true;
  try {
    // 首页：offset=0 + limit=PAGE_SIZE（等价旧 maxCount 语义，但前端翻页
    // 统一走 offset/limit 增量分页）。
    commits.value = await getCommitHistory(repoPath.value, undefined, 0, PAGE_SIZE);
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
  // 翻页中重复点击 / 已到尾部时直接忽略，避免并发请求把同一页追加两次。
  if (loading.value || !hasMore.value) return;
  loading.value = true;
  try {
    // GF-11：增量追加——只拉 [loaded, loaded+PAGE_SIZE) 这一页并 push，
    // 不再全量重取 (prevCount + PAGE_SIZE) 后整体替换（旧实现翻到第 k 页
    // 累计传输 O(k²)，且 history op 后 PAF-05 的「按钮只生效一次」陷阱）。
    const offset = commits.value.length;
    const more = await getCommitHistory(repoPath.value, undefined, offset, PAGE_SIZE);
    if (more.length > 0) {
      commits.value = [...commits.value, ...more];
    }
    // 短页（或 offset 已越过历史末端返回空）= 到顶，隐藏「加载更多」。
    hasMore.value = more.length >= PAGE_SIZE;
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
      loadResetPreview();
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
      loadResetPreview();
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

/** GF-17：加载 reset 结构化预演（只读；失败不阻塞确认流，执行时会再次校验）。 */
async function loadResetPreview() {
  const commit = resetDialog.commit;
  if (!commit || !repoPath.value) return;
  resetDialog.previewLoading = true;
  resetDialog.previewError = "";
  try {
    resetDialog.preview = await previewReset(repoPath.value, commit.oid, resetDialog.mode);
  } catch (e) {
    resetDialog.preview = null;
    resetDialog.previewError = errMsg(e);
  } finally {
    resetDialog.previewLoading = false;
  }
}

// 模式影响「不可恢复」判定（hard + 未提交变更），切换时重取预演。
watch(
  () => resetDialog.mode,
  () => {
    if (resetDialog.show) loadResetPreview();
  },
);

async function confirmReset() {
  const commit = resetDialog.commit;
  if (!commit) return;
  const mode = resetDialog.mode;
  const preview = resetDialog.preview;

  if (mode === "hard") {
    // Dangerous (§46): impact scope + data loss + recovery hint.
    // GF-17: 文案与结构化预演事实并列（提交数 / 变更数 / 不可恢复标注）。
    const impact = preview
      ? [
          `仓库：${repoPath.value}`,
          `当前分支：${preview.branch}`,
          `目标：${commit.shortOid}（${firstLine(commit.message)}）`,
          `将丢弃 ${preview.discardedCount} 个提交（${preview.discardedCommits
            .slice(0, 3)
            .map((c) => c.shortOid)
            .join(", ")}${preview.discardedCount > 3 ? " 等" : ""}）`,
          preview.lostChangesCount > 0
            ? `将丢弃 ${preview.lostChangesCount} 个未提交变更${preview.unrecoverable ? "（不可恢复，reflog 无法找回）" : ""}`
            : "工作区无未提交变更",
        ]
      : [
          `仓库：${repoPath.value}`,
          `当前分支：${currentBranchName()}`,
          `目标：${commit.shortOid}（${firstLine(commit.message)}）`,
        ];
    const confirmed = await new Promise<boolean>((resolve) => {
      dialog.error({
        title: "Reset --hard 确认（Dangerous）",
        content: `${impact.join("\n")}\n\n影响范围：HEAD、暂存区、工作区全部重置到该提交；未提交的更改将丢失，之后的提交将从分支上移除。\n保底：可先 Stash 保存现场；原 HEAD 位置会在执行结果中给出（可用 reflog 找回）。`,
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

/* GF-13b：+N 折叠标签可点击展开。 */
.branch-more-tag {
  cursor: pointer;
}

/* GF-13b：展开的其余分支（n-popover 内，n-scrollbar 限高滚动）。 */
.branch-overflow-list {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.graph-body {
  flex: 1;
  /* n-spin-content 非 flex 容器：flex:1 是死代码，显式 height:100% 才能把
     高度链贯通到 .commit-graph → .graph-scroll → VirtualList（GF-11）。 */
  height: 100%;
  overflow: hidden;
}

/* GF-11：高度链经 .n-spin-content 打通到 .graph-body，CommitGraph 内的
   VirtualList 才能拿到定高容器（同 F-18/F-20 模式；否则 .commit-graph
   的 height:100% 退化为 auto，全量直渲）。 */
.graph-spin {
  flex: 1;
  min-height: 0;
}

.graph-spin :deep(.n-spin-content) {
  height: 100%;
}

.commit-detail {
  padding: var(--gw-space-3);
}

.commit-detail-actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--gw-space-3);
  margin-top: var(--gw-space-3);
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
  /* PAF-17：改 soft 危险底（同 RuntimeDashboard 惯用法）——原背景与文字同为
     不透明 --gw-danger，冲突提示完全不可见；亮/暗主题下均为 danger 文字 +
     近透明底，对比度充足。 */
  background: color-mix(in srgb, var(--gw-danger) 12%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--gw-danger) 35%, transparent);
  font-size: 13px;
}

.conflict-text {
  flex: 1;
  color: var(--gw-danger);
  font-weight: 500;
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

/* GF-17：结构化预演区块（Roadmap §46 Repository/Branch/Files/Potential Data Loss） */
.reset-preview {
  display: block;
  margin-top: var(--gw-space-3);
  padding-top: var(--gw-space-3);
  border-top: 1px solid var(--gw-border);
  min-height: 24px;
}

.reset-preview-line {
  font-size: 13px;
  line-height: 1.8;
}

.reset-preview-list {
  margin: var(--gw-space-1) 0;
  padding-left: var(--gw-space-4);
  max-height: 140px;
  overflow-y: auto;
  font-size: 12px;
  line-height: 1.7;
}

.reset-preview-msg {
  margin-left: var(--gw-space-2);
  color: var(--gw-text-dim);
}

.reset-preview-error {
  font-size: 12px;
  color: var(--gw-warning, var(--gw-text-dim));
}

.danger-text {
  color: var(--gw-danger);
}

.dim-text {
  color: var(--gw-text-dim);
}

.mono {
  font-family: var(--gw-font-mono);
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
