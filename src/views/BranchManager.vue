<template>
  <div class="branch-manager">
    <!-- Header -->
    <div class="branch-header">
      <div class="repo-info">
        <span class="repo-path">{{ repoPath }}</span>
        <n-tag v-if="overview?.current" size="small" type="success">
          {{ overview.current }}
        </n-tag>
        <n-tag v-else-if="overview" size="small" type="warning">HEAD 游离</n-tag>
      </div>
      <n-button size="small" :loading="loading" @click="load">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
      <n-button size="small" type="primary" @click="handleCreate">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        新建分支
      </n-button>
      <n-button size="small" @click="openCompare()">
        <template #icon><n-icon><SwapHorizontalOutline /></n-icon></template>
        Compare
      </n-button>
      <n-button size="small" @click="rebaseDialogVisible = true">
        Rebase
      </n-button>
    </div>

    <!-- Operation state banners (T-15): resume an interrupted merge/rebase
         after reload or restart -->
    <div v-if="mergeInProgress" class="state-banner merge">
      <span class="banner-text">Merge 进行中：存在冲突待解决（MERGE_HEAD 已置）。</span>
      <n-button size="small" type="primary" dashed @click="handleMergeContinue">
        已解决，继续（Continue）
      </n-button>
      <n-button size="small" type="primary" dashed @click="openResolver">
        打开解决器
      </n-button>
      <n-button size="small" type="error" dashed @click="handleMergeAbort">
        中止（Abort）
      </n-button>
      <span class="banner-hint">请先在变更视图解决冲突并暂存（三方解决器随 T-16 提供）</span>
    </div>
    <div v-if="rebaseState" class="state-banner rebase">
      <span class="banner-text">
        Rebase 进行中：第 {{ rebaseState.position + 1 }}/{{ rebaseState.ops.length }} 步
        （onto {{ rebaseState.onto }}，当前 {{ currentOpLabel }}）。
      </span>
      <n-button size="small" type="primary" dashed @click="handleRebaseContinue">
        已解决，继续（Continue）
      </n-button>
      <n-button size="small" type="warning" dashed @click="handleRebaseSkip">
        跳过（Skip）
      </n-button>
      <n-button size="small" type="primary" dashed @click="openResolver">
        打开解决器
      </n-button>
      <n-button size="small" type="error" dashed @click="handleRebaseAbort">
        中止（Abort）
      </n-button>
    </div>

    <n-spin :show="loading">
      <div class="branch-body">
        <template v-if="overview">
          <!-- Local branches -->
          <Panel title="Local Branches（{{ overview.locals.length }}）" class="branch-section">
            <div
              v-for="b in overview.locals"
              :key="b.name"
              :class="['branch-row', { current: b.isCurrent }]"
            >
              <span class="branch-name">
                {{ b.name }}
                <n-tag v-if="b.isCurrent" size="small" type="success">当前</n-tag>
              </span>
              <span class="branch-track">
                <template v-if="b.upstream">
                  <span class="upstream">{{ b.upstream }}</span>
                  <span v-if="b.ahead > 0" class="ahead">↑{{ b.ahead }}</span>
                  <span v-if="b.behind > 0" class="behind">↓{{ b.behind }}</span>
                </template>
                <span v-else class="no-upstream">无上游</span>
              </span>
              <span class="branch-commit" :title="b.lastCommitOid">
                {{ shortOid(b.lastCommitOid) }} {{ b.lastCommitMessage }}
              </span>
              <n-dropdown trigger="click" :options="localBranchOptions(b)" @select="(key: string) => handleLocalCommand(key, b)">
                <n-button size="small" text>
                  <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                </n-button>
              </n-dropdown>
            </div>
            <n-empty v-if="overview.locals.length === 0" description="无本地分支" />
          </Panel>

          <!-- Remote branches -->
          <Panel title="Remote Branches（{{ overview.remotes.length }}）" class="branch-section">
            <div v-for="r in overview.remotes" :key="r.name" class="branch-row">
              <span class="branch-name">{{ r.name }}</span>
              <span class="branch-track" />
              <span class="branch-commit" :title="r.lastCommitOid">
                {{ shortOid(r.lastCommitOid) }} {{ r.lastCommitMessage }}
              </span>
              <n-dropdown trigger="click" :options="remoteBranchOptions()" @select="(key: string) => handleRemoteCommand(key, r)">
                <n-button size="small" text>
                  <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                </n-button>
              </n-dropdown>
            </div>
            <n-empty v-if="overview.remotes.length === 0" description="无远程分支" />
          </Panel>

          <!-- Tags -->
          <Panel title="Tags（{{ overview.tags.length }}）" class="branch-section">
            <div v-for="t in overview.tags" :key="t.name" class="branch-row">
              <span class="branch-name">{{ t.name }}</span>
              <span class="branch-track tag-message" :title="t.message ?? ''">{{ t.message ?? "" }}</span>
              <span class="branch-commit" :title="t.targetOid">{{ shortOid(t.targetOid) }}</span>
              <span />
            </div>
            <n-empty v-if="overview.tags.length === 0" description="无标签" />
          </Panel>
        </template>
      </div>
    </n-spin>

    <!-- Merge dialog (T-15) -->
    <n-modal v-model:show="mergeDialog.show" preset="card" title="Merge 到当前分支" style="width: 520px">
      <div class="merge-form">
        <div class="merge-line">
          源分支：<strong>{{ mergeDialog.branch }}</strong> → 当前分支：<strong>{{ overview?.current }}</strong>
        </div>
        <n-radio-group v-model:value="mergeDialog.mode" class="merge-modes">
          <n-radio value="normal">普通（可快进则快进）</n-radio>
          <n-radio value="no-ff">--no-ff（始终生成合并提交）</n-radio>
          <n-radio value="squash">--squash（压成暂存更改，不产生合并提交）</n-radio>
        </n-radio-group>
      </div>
      <template #footer>
        <n-button @click="mergeDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="mergeDialog.loading" @click="runMerge">
          执行 Merge
        </n-button>
      </template>
    </n-modal>

    <!-- Interactive Rebase dialog (T-15) -->
    <RebaseDialog
      v-model="rebaseDialogVisible"
      :repo-path="repoPath"
      :revisions="rebaseRevisions"
      :default-onto="defaultOnto"
      @finished="onRebaseFinished"
    />

    <!-- Compare dialog -->
    <n-modal v-model:show="compare.show" preset="card" title="Branch Compare" style="width: 80%; margin-top: 5vh">
      <div class="compare-form">
        <n-select v-model:value="compare.base" filterable placeholder="Base（基准）" style="width: 240px" :options="revisionSelectOptions" />
        <span class="compare-arrow">⇄</span>
        <n-select v-model:value="compare.other" filterable placeholder="Other（对比）" style="width: 240px" :options="revisionSelectOptions" />
        <n-button
          type="primary"
          :loading="compare.loading"
          :disabled="!compare.base || !compare.other"
          @click="runCompare"
        >
          比较
        </n-button>
      </div>

      <div v-if="compare.result" class="compare-result">
        <div class="compare-summary">
          <n-tag type="success">领先 {{ compare.result.ahead.length }}</n-tag>
          <span class="summary-text">{{ compare.result.other }} 领先 {{ compare.result.base }}</span>
          <n-tag type="warning">落后 {{ compare.result.behind.length }}</n-tag>
          <span class="summary-text">{{ compare.result.other }} 落后 {{ compare.result.base }}</span>
        </div>
        <n-tabs v-model:value="compare.tab">
          <n-tab-pane :tab="`领先 Commits（${compare.result.ahead.length}）`" name="ahead">
            <div v-for="c in compare.result.ahead" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <n-empty v-if="compare.result.ahead.length === 0" description="无" />
          </n-tab-pane>
          <n-tab-pane :tab="`落后 Commits（${compare.result.behind.length}）`" name="behind">
            <div v-for="c in compare.result.behind" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <n-empty v-if="compare.result.behind.length === 0" description="无" />
          </n-tab-pane>
          <n-tab-pane :tab="`文件差异（${compare.result.files.length}）`" name="files">
            <div class="compare-files">
              <div class="file-list">
                <div
                  v-for="f in compare.result.files"
                  :key="f.newPath"
                  :class="['file-item', { active: compare.selectedFile?.newPath === f.newPath }]"
                  @click="compare.selectedFile = f"
                >
                  <span :class="['file-status-icon', f.status]">{{ statusIcon(f.status) }}</span>
                  <span class="file-name">{{ f.newPath }}</span>
                </div>
                <n-empty v-if="compare.result.files.length === 0" description="无文件差异" />
              </div>
              <div class="file-diff">
                <UnifiedDiff v-if="compare.selectedFile" :file="compare.selectedFile" />
                <n-empty v-else description="选择文件查看 Diff" />
              </div>
            </div>
          </n-tab-pane>
        </n-tabs>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { EllipsisVerticalOutline, AddOutline, RefreshOutline, SwapHorizontalOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import {
  checkoutBranch,
  compareBranches,
  createBranch,
  deleteBranch,
  listBranches,
  pushBranch,
  renameBranch,
  setUpstream,
  trackRemoteBranch,
} from "@/api/branch";
import type { BranchEntry, BranchOverview, CompareResult, RemoteBranchEntry } from "@/types/branch";
import type { FileDiff } from "@/types/git";
import { syncPull } from "@/api/git_ops";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import RebaseDialog from "@/components/branch/RebaseDialog.vue";
import Panel from "@/components/shell/Panel.vue";
import { getMergeInProgress, mergeAbort, mergeBranch, mergeContinue } from "@/api/merge";
import { getRebaseState, rebaseAbort, rebaseContinue, rebaseSkip } from "@/api/rebase";
import type { MergeOutcome } from "@/types/merge";
import type { RebaseOutcome, RebaseState } from "@/types/rebase";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();

const repoPath = ref("");
const overview = ref<BranchOverview | null>(null);
const loading = ref(false);

// --- T-15 merge / rebase state ---
const mergeDialog = reactive({ show: false, branch: "", mode: "normal", loading: false });
const rebaseDialogVisible = ref(false);
const mergeInProgress = ref(false);
const rebaseState = ref<RebaseState | null>(null);

/** Onto candidates: every local branch except the current one, plus remotes. */
const rebaseRevisions = computed<string[]>(() => {
  if (!overview.value) return [];
  return [
    ...overview.value.locals.filter((b) => !b.isCurrent).map((b) => b.name),
    ...overview.value.remotes.map((r) => r.name),
  ];
});

const defaultOnto = computed<string>(() => {
  const opts = rebaseRevisions.value;
  return (
    opts.find((n) => n === "master") ??
    opts.find((n) => n === "main") ??
    opts[0] ??
    ""
  );
});

const currentOpLabel = computed<string>(() => {
  const s = rebaseState.value;
  if (!s || s.position >= s.ops.length) return "—";
  const op = s.ops[s.position];
  return `${op.action} ${op.oid.slice(0, 7)} ${op.subject}`;
});

const compare = reactive<{
  show: boolean;
  base: string;
  other: string;
  loading: boolean;
  result: CompareResult | null;
  tab: "ahead" | "behind" | "files";
  selectedFile: FileDiff | null;
}>({
  show: false,
  base: "",
  other: "",
  loading: false,
  result: null,
  tab: "ahead",
  selectedFile: null,
});

/** Compare revisions: local + remote branch names and tags. */
const revisionOptions = computed<string[]>(() => {
  if (!overview.value) return [];
  return [
    ...overview.value.locals.map((b) => b.name),
    ...overview.value.remotes.map((r) => r.name),
    ...overview.value.tags.map((t) => t.name),
  ];
});

/** n-select compatible options array for revision selectors. */
const revisionSelectOptions = computed(() =>
  revisionOptions.value.map((o) => ({ label: o, value: o })),
);

/** n-dropdown options for local branch rows. */
function localBranchOptions(b: BranchEntry) {
  const opts: { label: string; key: string; props?: Record<string, unknown> }[] = [];
  if (!b.isCurrent) opts.push({ label: "Checkout", key: "checkout" });
  opts.push({ label: "Rename", key: "rename" });
  opts.push({ label: "Set Upstream", key: "set-upstream" });
  if (b.isCurrent) opts.push({ label: "Pull（--ff-only）", key: "pull" });
  opts.push({ label: "Push", key: "push" });
  opts.push({ label: "Compare", key: "compare" });
  opts.push({ label: "Merge 到当前分支…", key: "merge" });
  if (!b.isCurrent) {
    opts.push({ type: "divider", key: "d1" } as never);
    opts.push({ label: "Delete", key: "delete", props: { style: "color: var(--gw-danger)" } });
  }
  return opts;
}

/** n-dropdown options for remote branch rows. */
function remoteBranchOptions() {
  return [
    { label: "Track（检出为本地分支）", key: "track" },
    { label: "Compare", key: "compare" },
  ];
}

onMounted(async () => {
  const repo = route.query.repo as string;
  if (!repo) {
    message.warning("未指定仓库路径");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await load();
});

async function load() {
  loading.value = true;
  try {
    overview.value = await listBranches(repoPath.value);
    // Resume surface for an interrupted merge/rebase (T-15 restart recovery).
    mergeInProgress.value = await getMergeInProgress(repoPath.value);
    rebaseState.value = await getRebaseState(repoPath.value);
  } catch (e) {
    message.error("获取分支列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function openResolver() {
  router.push({ name: "conflict-resolver", query: { repo: repoPath.value } });
}

function shortOid(oid: string): string {
  return oid ? oid.slice(0, 7) : "";
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
// Local branch commands (§46: Delete = Dangerous 二次确认; Push = Warning 确认)
// ---------------------------------------------------------------------------

async function handleLocalCommand(cmd: string, b: BranchEntry) {
  switch (cmd) {
    case "checkout":
      await runOp(`已切换到分支 ${b.name}`, () => checkoutBranch(repoPath.value, b.name));
      break;
    case "rename":
      await handleRename(b);
      break;
    case "set-upstream":
      await handleSetUpstream(b);
      break;
    case "push":
      await handlePush(b);
      break;
    case "pull":
      // --ff-only pull onto the current branch; divergent state fails safely.
      await runOp("Pull 完成", () => syncPull(repoPath.value));
      break;
    case "compare":
      openCompare(b.name);
      break;
    case "merge":
      mergeDialog.branch = b.name;
      mergeDialog.mode = "normal";
      mergeDialog.show = true;
      break;
    case "delete":
      await handleDelete(b);
      break;
  }
}

async function handleRemoteCommand(cmd: string, r: RemoteBranchEntry) {
  switch (cmd) {
    case "track":
      await runOp(`已创建跟踪分支（${r.name}）`, () =>
        trackRemoteBranch(repoPath.value, r.name),
      );
      break;
    case "compare":
      openCompare(r.name);
      break;
  }
}

/** Run an op, toast the result, reload on success. */
async function runOp(successMsg: string, op: () => Promise<unknown>) {
  try {
    await op();
    message.success(successMsg);
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleCreate() {
  try {
    const name = await prompt(dialog, {
      title: "新建分支",
      content: "新分支名称（基于当前 HEAD；可在下方输入框留空目标）",
      confirmText: "创建",
      cancelText: "取消",
      pattern: /^[^\s~^:?*[\]\\]+$/,
      patternError: "分支名不合法",
    });
    if (!name) return;
    await runOp(`已创建分支 ${name}`, () => createBranch(repoPath.value, name));
  } catch (e) {
    if (e !== "cancel") message.error("创建分支失败: " + errMsg(e));
  }
}

async function handleRename(b: BranchEntry) {
  try {
    const newName = await prompt(dialog, {
      title: "Rename Branch",
      content: `将分支 ${b.name} 重命名为：`,
      confirmText: "重命名",
      cancelText: "取消",
      defaultValue: b.name,
      pattern: /^[^\s~^:?*[\]\\]+$/,
      patternError: "分支名不合法",
    });
    if (!newName || newName === b.name) return;
    await runOp(`已重命名为 ${newName}`, () =>
      renameBranch(repoPath.value, b.name, newName),
    );
  } catch (e) {
    if (e !== "cancel") message.error("重命名失败: " + errMsg(e));
  }
}

async function handleSetUpstream(b: BranchEntry) {
  if (!overview.value) return;
  const options = overview.value.remotes.map((r) => r.name);
  try {
    const value = await prompt(dialog, {
      title: "Set Upstream",
      content: `设置 ${b.name} 的上游（输入远程分支名，如 origin/main；输入 "-" 清除上游）：`,
      confirmText: "确定",
      cancelText: "取消",
      defaultValue: b.upstream ?? (options.length === 1 ? options[0] : ""),
      pattern: /^.+$/,
      patternError: "必须是现有远程分支名，或 - 清除",
    });
    // Manual validation for custom logic
    if (value !== "-" && !options.includes(value)) {
      message.error("必须是现有远程分支名，或 - 清除");
      return;
    }
    const upstream = value === "-" ? undefined : value;
    await runOp(
      upstream ? `已设置上游 ${upstream}` : "已清除上游",
      () => setUpstream(repoPath.value, b.name, upstream),
    );
  } catch (e) {
    if (e !== "cancel") message.error("设置上游失败: " + errMsg(e));
  }
}

async function handlePush(b: BranchEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Push 确认",
        content: `推送本地分支 ${b.name} 到 ${b.upstream ?? "默认远程"}？（不启用 force）`,
        positiveText: "Push",
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
    const output = await pushBranch(repoPath.value, b.name);
    message.success(output ? `Push 完成：${output}` : "Push 完成");
    await load();
  } catch (e) {
    message.error("Push 失败: " + errMsg(e));
  }
}

/** Dangerous op (§46): 二次确认；未合入时升级为强制删除确认。 */
async function handleDelete(b: BranchEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Delete 确认（Dangerous）",
        content: `确认删除本地分支 ${b.name}？此操作不可撤销（可用 reflog 尝试找回）。`,
        positiveText: "删除",
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
    await deleteBranch(repoPath.value, b.name);
    message.success(`已删除分支 ${b.name}`);
    await load();
  } catch (e) {
    const msg = errMsg(e);
    if (msg.includes("not fully merged")) {
      // Second gate: force-delete confirmation for unmerged branches.
      try {
        await new Promise<void>((resolve, reject) => {
          dialog.error({
            title: "强制删除确认（Dangerous）",
            content: `分支 ${b.name} 未完全合入当前 HEAD，删除可能丢失提交。确认强制删除？`,
            positiveText: "强制删除",
            negativeText: "取消",
            onPositiveClick: () => resolve(),
            onNegativeClick: () => reject("cancel"),
            onClose: () => reject("cancel"),
          });
        });
      } catch {
        return;
      }
      await runOp(`已强制删除分支 ${b.name}`, () =>
        deleteBranch(repoPath.value, b.name, true),
      );
    } else {
      message.error("删除失败: " + msg);
    }
  }
}

// ---------------------------------------------------------------------------
// Merge / Rebase (T-15): Warning 确认 + 中断恢复
// ---------------------------------------------------------------------------

async function runMerge() {
  const { branch, mode } = mergeDialog;
  // Warning-level confirm (§46): history-changing op with impact scope.
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Merge 确认（Warning）",
        content: `仓库：${repoPath.value}\n将把分支 ${branch} 合并到当前分支 ${overview.value?.current ?? "HEAD"}（模式：${mode}）。\n若产生冲突，仓库会进入 Merge 状态，可解决后继续或中止恢复。`,
        positiveText: "执行 Merge",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }

  mergeDialog.loading = true;
  try {
    const outcome = await mergeBranch(repoPath.value, branch, mode);
    mergeDialog.show = false;
    handleMergeOutcome(outcome);
    await load();
  } catch (e) {
    message.error("Merge 失败: " + errMsg(e));
  } finally {
    mergeDialog.loading = false;
  }
}

function handleMergeOutcome(outcome: MergeOutcome) {
  switch (outcome.status) {
    case "upToDate":
      message.info("已是最新，无需合并");
      break;
    case "fastForward":
      message.success(`已快进到 ${outcome.to.slice(0, 7)}`);
      break;
    case "merged":
      message.success(`合并完成（${outcome.commitOid.slice(0, 7)}）`);
      break;
    case "squashed":
      message.success("Squash 结果已暂存，请在变更视图提交");
      break;
    case "conflict":
      message.warning(
        `合并冲突（${outcome.files.length} 个文件）：${outcome.files.join("、")}。请在变更视图解决后回来 Continue，或 Abort 恢复。`,
      );
      break;
  }
}

async function handleMergeContinue() {
  try {
    const oid = await mergeContinue(repoPath.value);
    message.success(`Merge 已完成（${oid.slice(0, 7)}）`);
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleMergeAbort() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Merge Abort 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n将放弃本次合并并恢复到合并前状态（hard reset），冲突中的修改将丢失。`,
        positiveText: "中止并恢复",
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
    await mergeAbort(repoPath.value);
    message.success("已中止 Merge 并恢复");
    await load();
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

async function onRebaseFinished(outcome: RebaseOutcome | null) {
  if (!outcome) return;
  if (outcome.status === "success") {
    message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
  } else {
    message.warning(
      `Rebase 在第 ${outcome.position + 1}/${outcome.total} 步冲突（${outcome.files.join("、")}）。解决后可 Continue / Skip / Abort。`,
    );
  }
  await load();
}

async function handleRebaseContinue() {
  try {
    const outcome = await rebaseContinue(repoPath.value);
    if (outcome.status === "success") {
      message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
    } else {
      message.warning(
        `第 ${outcome.position + 1}/${outcome.total} 步再次冲突：${outcome.files.join("、")}`,
      );
    }
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleRebaseSkip() {
  try {
    const outcome = await rebaseSkip(repoPath.value);
    if (outcome.status === "success") {
      message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
    } else {
      message.warning(
        `第 ${outcome.position + 1}/${outcome.total} 步再次冲突：${outcome.files.join("、")}`,
      );
    }
    await load();
  } catch (e) {
    message.error("Skip 失败: " + errMsg(e));
  }
}

async function handleRebaseAbort() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Rebase Abort 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n将放弃本次 Rebase 并恢复到 rebase 前位置（hard reset 到 ${rebaseState.value?.originalHead.slice(0, 7) ?? "?"}），进行中的修改将丢失。`,
        positiveText: "中止并恢复",
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
    await rebaseAbort(repoPath.value);
    message.success("已中止 Rebase 并恢复");
    await load();
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

// ---------------------------------------------------------------------------
// Compare
// ---------------------------------------------------------------------------

function openCompare(presetOther?: string) {
  compare.base = overview.value?.current ?? overview.value?.locals[0]?.name ?? "";
  compare.other = presetOther ?? "";
  compare.result = null;
  compare.selectedFile = null;
  compare.tab = "ahead";
  compare.show = true;
}

async function runCompare() {
  compare.loading = true;
  compare.selectedFile = null;
  try {
    compare.result = await compareBranches(repoPath.value, compare.base, compare.other);
    compare.tab = "ahead";
  } catch (e) {
    message.error("Compare 失败: " + errMsg(e));
  } finally {
    compare.loading = false;
  }
}
</script>

<style scoped>
.branch-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.branch-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.repo-info {
  flex: 1;
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  min-width: 0;
}

.repo-path {
  font-size: 14px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--gw-space-3) var(--gw-space-4);
  background: var(--gw-bg-hover);
}

/* D-10：section 外壳已替换为 Panel 组件；间距沿用原列表节奏 */
.branch-section {
  margin-bottom: 12px;
}

/* Panel 标题行下加分隔线，对齐原 .section-title 视觉 */
.branch-section :deep(.panel-header) {
  border-bottom: 1px solid var(--gw-border);
}

.branch-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 6px 12px;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.branch-row:last-child {
  border-bottom: none;
}

.branch-row.current {
  background: var(--gw-bg-hover);
}

.branch-name {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 220px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
  flex-shrink: 0;
}

.branch-track {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 200px;
  flex-shrink: 0;
}

.upstream {
  color: var(--gw-text-dim);
}

.ahead {
  color: var(--gw-success);
  font-weight: 600;
}

.behind {
  color: var(--gw-warning);
  font-weight: 600;
}

.no-upstream {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.tag-message {
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-commit {
  flex: 1;
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.danger-item {
  color: var(--gw-danger);
}

.compare-form {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  margin-bottom: 12px;
}

.compare-arrow {
  color: var(--gw-text-dim);
}

.compare-summary {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-bottom: 8px;
}

.summary-text {
  color: var(--gw-text-dim);
  font-size: 13px;
  margin-right: 8px;
}

.commit-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-border);
}

.commit-oid {
  font-family: var(--gw-font-mono);
  color: var(--gw-accent);
  width: 70px;
  flex-shrink: 0;
}

.commit-msg {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-meta {
  color: var(--gw-text-dim);
  font-size: 12px;
  flex-shrink: 0;
}

.compare-files {
  display: flex;
  height: 50vh;
  border: 1px solid var(--gw-border);
}

.file-list {
  width: 260px;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  cursor: pointer;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-border);
}

.file-item:hover {
  background: var(--gw-bg-hover);
}

.file-item.active {
  background: var(--gw-bg-hover);
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
  font-family: var(--gw-font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-diff {
  flex: 1;
  overflow: hidden;
}
</style>

<style scoped>
.state-banner {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 16px;
  font-size: 13px;
  border-bottom: 1px solid var(--gw-danger);
}

.state-banner.merge {
  background: var(--gw-warning);
  border-bottom-color: var(--gw-warning);
}

.state-banner.rebase {
  background: var(--gw-danger);
}

.banner-text {
  font-weight: 500;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.banner-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.merge-line {
  margin-bottom: 12px;
  font-size: 13px;
  color: var(--gw-text-dim);
}

.merge-modes {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}
</style>
