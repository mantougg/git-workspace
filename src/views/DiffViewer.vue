<template>
  <div class="diff-viewer">
    <!-- Header -->
    <div class="diff-header">
      <div class="repo-info">
        <span class="repo-path">{{ repoPath }}</span>
        <n-tag v-if="files.length > 0" size="small">
          {{ files.length }} 个文件变更
        </n-tag>
      </div>
      <n-radio-group v-model:value="source" size="small">
        <n-radio-button value="unstaged">未暂存</n-radio-button>
        <n-radio-button value="staged">已暂存</n-radio-button>
        <n-radio-button value="compare">比较</n-radio-button>
      </n-radio-group>
      <n-tag v-if="source === 'commit'" size="small" type="warning">
        提交 {{ commitOid.slice(0, 7) }}
      </n-tag>
      <div class="diff-options">
        <n-checkbox v-model:checked="diffOptions.ignoreWhitespace">
          Ignore Whitespace
        </n-checkbox>
        <n-checkbox v-model:checked="diffOptions.ignoreWhitespaceEol">
          Ignore EOL
        </n-checkbox>
        <n-checkbox v-model:checked="diffOptions.ignoreCase">
          Ignore Case
        </n-checkbox>
      </div>
      <n-radio-group v-model:value="diffMode" size="small">
        <n-radio-button value="unified">Unified</n-radio-button>
        <n-radio-button value="side-by-side">Side by Side</n-radio-button>
      </n-radio-group>
      <n-button
        v-if="source === 'staged' && files.length > 0"
        type="success"
        size="small"
        @click="stagedCommitDialog.show = true"
      >
        提交暂存区
      </n-button>
      <n-button
        v-if="files.length > 0"
        type="primary"
        size="small"
        @click="gitAssistantVisible = true"
      >
        <template #icon><n-icon><SparklesOutline /></n-icon></template>
        AI Assistant
      </n-button>
      <n-button v-if="files.length > 0" size="small" @click="openDiffAssistant">
        <template #icon><n-icon><SparklesOutline /></n-icon></template>
        Assistant 会话
      </n-button>
    </div>

    <!-- Commit staged area dialog (T-11 + T-12) -->
    <n-modal v-model:show="stagedCommitDialog.show" preset="card" title="提交暂存区" style="width: 480px">
      <n-input
        v-model:value="stagedCommitDialog.message"
        type="textarea"
        :rows="3"
        placeholder="请输入 commit message（Amend 可留空 = --no-edit）"
      />
      <div class="staged-commit-options">
        <n-checkbox v-model:checked="stagedCommitDialog.amend" size="small">
          Amend 上次提交
        </n-checkbox>
        <n-checkbox v-model:checked="stagedCommitDialog.thenPush" size="small">
          提交后 Push
        </n-checkbox>
      </div>
      <template #footer>
        <n-button @click="stagedCommitDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="stagedCommitDialog.loading"
          @click="handleStagedCommit"
        >
          提交
        </n-button>
      </template>
    </n-modal>

    <!-- Pre-commit safety findings dialog (T-11 §5) -->
    <n-modal v-model:show="stagedScanDialog.show" preset="card" title="提交安全检查" style="width: 560px">
      <n-alert
        type="warning"
        :closable="false"
        :show-icon="true"
        title="发现以下风险项，确认无误后可放行提交："
      />
      <ul class="scan-finding-list">
        <li v-for="(f, i) in stagedScanDialog.findings" :key="i">
          <n-tag
            size="small"
            :type="f.kind === 'forbidden' ? 'error' : 'warning'"
          >
            {{ f.kind }}
          </n-tag>
          <span class="scan-path">{{ f.path }}</span>
          <span class="scan-detail">{{ f.detail }}</span>
        </li>
      </ul>
      <template #footer>
        <n-button @click="stagedScanDialog.show = false">取消</n-button>
        <n-button type="error" @click="handleStagedCommitOverride">
          仍要提交
        </n-button>
      </template>
    </n-modal>

    <!-- Compare bar (T-12 revision diff) -->
    <div v-if="source === 'compare'" class="compare-bar">
      <n-input
        v-model:value="compareBase"
        size="small"
        placeholder="base（分支 / 标签 / 提交）"
        style="width: 240px"
        @keyup.enter="loadDiff"
      />
      <span class="compare-sep">↔</span>
      <n-input
        v-model:value="compareOther"
        size="small"
        placeholder="other（分支 / 标签 / 提交）"
        style="width: 240px"
        @keyup.enter="loadDiff"
      />
      <n-button size="small" type="primary" @click="loadDiff">
        对比
      </n-button>
    </div>

    <!-- AI Review result dialog -->
    <n-modal
      v-model:show="showReview"
      preset="card"
      title="AI Code Review"
      style="width: 600px"
      :mask-closable="false"
    >
      <div v-if="reviewResult" class="review-result">
        <div class="review-summary">
          <strong>Summary:</strong> {{ reviewResult.summary }}
        </div>
        <div v-if="reviewResult.issues.length > 0" class="review-issues">
          <div
            v-for="(issue, i) in reviewResult.issues"
            :key="i"
            class="review-issue"
          >
            <n-tag
              :type="issue.severity === 'high' ? 'error' : issue.severity === 'medium' ? 'warning' : 'info'"
              size="small"
            >
              {{ issue.severity }}
            </n-tag>
            <n-tag size="small" :bordered="false">
              {{ issue.category }}
            </n-tag>
            <span class="issue-file">{{ issue.file }}</span>
            <div class="issue-desc">{{ issue.description }}</div>
          </div>
        </div>
        <n-empty v-else description="No issues found" />
      </div>
    </n-modal>

    <AiGitAssistantDialog
      v-model="gitAssistantVisible"
      :repositories="assistantRepositories"
      :source="assistantSource"
      @apply-commit-suggestion="applyAiCommitSuggestion"
    />

    <!-- Body -->
    <n-spin :show="loading">
      <div class="diff-body">
        <template v-if="files.length > 0">
          <!-- File list -->
          <div class="file-list">
            <div
              v-for="file in files"
              :key="file.newPath"
              :class="['file-item', { active: selectedFile?.newPath === file.newPath }]"
              @click="selectedFile = file"
            >
              <span :class="['file-status-icon', file.status]">
                {{ statusIcon(file.status) }}
              </span>
              <span class="file-name">{{ file.newPath }}</span>
            </div>
          </div>

          <!-- Diff content -->
          <div class="diff-content">
            <div v-if="selectedFile" class="file-diff">
              <div class="file-diff-header">
                <span class="file-diff-path">{{ selectedFile.newPath }}</span>
                <n-button
                  v-if="source === 'unstaged' && selectedFile.status === 'untracked'"
                  size="small"
                  :loading="stageLoading"
                  @click="stageWholeFile"
                >
                  暂存整个文件
                </n-button>
                <n-tag
                  v-if="ignoreOptionsActive && (source === 'unstaged' || source === 'staged')"
                  size="small"
                  type="info"
                >
                  Ignore 选项开启时暂存操作不可用
                </n-tag>
              </div>
              <div class="file-diff-body">
                <UnifiedDiff
                  v-if="diffMode === 'unified'"
                  :file="selectedFile"
                  :mode="interactiveMode"
                  @op="handleStageOp"
                />
                <SideBySideDiff v-else :file="selectedFile" />
              </div>
            </div>
            <n-empty v-else description="选择文件查看 Diff" />
          </div>
        </template>
        <n-empty v-else-if="!loading" :description="emptyText" />
      </div>
    </n-spin>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import { SparklesOutline } from "@vicons/ionicons5";
import { useMessage } from "naive-ui";
import {
  getStagedDiff,
  getUnstagedDiff,
  getRevisionDiff,
  getCommitDiff,
  stageHunk,
  unstageHunk,
  stageLines,
  unstageLines,
  type DiffOptions,
} from "@/api/git";
import { batchAdd } from "@/api/changes";
import { batchCommit } from "@/api/git_ops";
import { scanCommit } from "@/api/commit";
import type { CommitScanFinding } from "@/types/commit";
import type { StageOp } from "@/components/diff/UnifiedDiff.vue";
import type { ReviewResult } from "@/types/ai";
import type { FileDiff } from "@/types/git";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import SideBySideDiff from "@/components/diff/SideBySideDiff.vue";
import AiGitAssistantDialog from "@/components/ai/AiGitAssistantDialog.vue";
import { useAiAssistant } from "@/composables/useAiAssistant";
import { errMsg } from "@/utils/error";
import { startFrameMeter, type FrameStats } from "@/utils/frameTime";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const { resolveCurrentRepo } = useCurrentRepo();
const { openAssistant } = useAiAssistant();

const repoPath = ref("");
const files = ref<FileDiff[]>([]);
const selectedFile = ref<FileDiff | null>(null);
const loading = ref(false);
const diffMode = ref<"unified" | "side-by-side">("unified");
// Diff source mode (T-12): workdir views offer hunk/line staging;
// compare/commit are read-only revision diffs.
const source = ref<"unstaged" | "staged" | "compare" | "commit">(
  "unstaged",
);
const compareBase = ref("");
const compareOther = ref("");
const commitOid = ref("");
const stageLoading = ref(false);
const stagedCommitDialog = ref({
  show: false,
  loading: false,
  message: "",
  amend: false,
  thenPush: false,
});
const stagedScanDialog = ref({ show: false, findings: [] as CommitScanFinding[] });
const diffOptions = ref<DiffOptions>({
  ignoreWhitespace: false,
  ignoreWhitespaceEol: false,
  ignoreCase: false,
});
const showReview = ref(false);
const reviewResult = ref<ReviewResult | null>(null);
const gitAssistantVisible = ref(false);
const assistantRepositories = computed(() => repoPath.value ? [{
  repoPath: repoPath.value,
  name: repoPath.value.split(/[\\/]/).filter(Boolean).pop() ?? "repository",
  files: files.value.map((file) => file.newPath || file.oldPath),
}] : []);
const assistantSource = computed<"workdir" | "staged" | "unstaged">(
  () => source.value === "staged" ? "staged" : source.value === "unstaged" ? "unstaged" : "workdir",
);

function openDiffAssistant() {
  if (!repoPath.value) return;
  const paths = files.value.map((file) => file.newPath || file.oldPath).filter(Boolean);
  openAssistant({
    repositoryPaths: [repoPath.value],
    inferredRole: "gitReviewer",
    origin: `Diff · ${paths.length} 个文件`,
    supplementary: [{
      role: "userNote",
      kind: "diff",
      sourceId: `diff:${repoPath.value}:${source.value}`,
      displayName: `当前 Diff 文件列表（${paths.length}）`,
      content: paths.join("\n"),
    }],
    draft: "请解释当前 Diff 的主要变更、风险和建议。",
  });
}

onMounted(async () => {
  stopFrameMeter = startFrameMeter("diff-viewer");
  // F-14/F-17：repo 走 query → 全局当前仓库 → 工作区首仓库兜底；
  // commit/base/other 仍为任务页必带 query（此页不进 SideNav，正常不会无参直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  const commit = route.query.commit as string | undefined;
  const base = route.query.base as string | undefined;
  const other = route.query.other as string | undefined;
  if (commit) {
    commitOid.value = commit;
    source.value = "commit";
  } else if (base && other) {
    compareBase.value = base;
    compareOther.value = other;
    source.value = "compare";
  }
  await loadDiff();
});

let stopFrameMeter: (() => FrameStats) | null = null;

// Frame-time measurement channel for the diff rendering budget (T-04):
// samples rAF frame durations while this view is open; live stats are on
// window.__gwPerf["diff-viewer"], slow frames are warned in the console.
onUnmounted(() => {
  const stats = stopFrameMeter?.();
  if (stats && stats.frames > 0) {
    console.debug(
      `[perf:diff-viewer] frames=${stats.frames} avg=${stats.avgMs}ms p95=${stats.p95Ms}ms max=${stats.maxMs}ms`,
    );
  }
});

let loadSeq = 0;

async function loadDiff() {
  if (
    source.value === "compare" &&
    (!compareBase.value.trim() || !compareOther.value.trim())
  ) {
    files.value = [];
    selectedFile.value = null;
    return;
  }
  const seq = ++loadSeq;
  loading.value = true;
  try {
    const next = await fetchBySource();
    if (seq !== loadSeq) return; // 已有更新的请求，丢弃过期结果
    files.value = next;
    // 保留当前选中文件，避免切换设置时跳回第一个文件
    const currentPath = selectedFile.value?.newPath;
    selectedFile.value =
      next.find((f) => f.newPath === currentPath) ?? next[0] ?? null;
  } catch (e) {
    if (seq !== loadSeq) return;
    message.error("获取 Diff 失败: " + errMsg(e));
  } finally {
    if (seq === loadSeq) loading.value = false;
  }
}

// 切换 Ignore 设置时即时重新计算 diff（Roadmap §9）
watch(diffOptions, () => loadDiff(), { deep: true });

function fetchBySource(): Promise<FileDiff[]> {
  switch (source.value) {
    case "staged":
      return getStagedDiff(repoPath.value, diffOptions.value);
    case "compare":
      return getRevisionDiff(
        repoPath.value,
        compareBase.value.trim(),
        compareOther.value.trim(),
        diffOptions.value,
      );
    case "commit":
      return getCommitDiff(repoPath.value, commitOid.value, diffOptions.value);
    default:
      return getUnstagedDiff(repoPath.value, diffOptions.value);
  }
}

watch(source, () => loadDiff());

const ignoreOptionsActive = computed(
  () =>
    diffOptions.value.ignoreWhitespace ||
    diffOptions.value.ignoreWhitespaceEol ||
    diffOptions.value.ignoreCase,
);

// Hunk/line staging is only offered in the workdir views with default
// diff options: Ignore options renumber hunks/lines, which would break
// the indices staging operates on (T-12 contract).
const interactiveMode = computed<"stage" | "unstage" | null>(() => {
  if (ignoreOptionsActive.value) return null;
  if (source.value === "unstaged") return "stage";
  if (source.value === "staged") return "unstage";
  return null;
});

const emptyText = computed(() => {
  switch (source.value) {
    case "staged":
      return "暂存区为空";
    case "compare":
      return "两个引用之间没有差异（或尚未输入比较对象）";
    case "commit":
      return "该提交没有文件变更";
    default:
      return "没有未暂存的变更";
  }
});

async function handleStageOp(op: StageOp) {
  const file = selectedFile.value;
  if (!file) return;
  const filePath = file.newPath || file.oldPath;
  stageLoading.value = true;
  try {
    if (source.value === "unstaged") {
      if (op.kind === "hunk") {
        await stageHunk(repoPath.value, filePath, op.hunkIndex);
      } else {
        await stageLines(
          repoPath.value,
          filePath,
          op.hunkIndex,
          op.lineIndices ?? [],
        );
      }
      message.success("已暂存");
    } else if (source.value === "staged") {
      if (op.kind === "hunk") {
        await unstageHunk(repoPath.value, filePath, op.hunkIndex);
      } else {
        await unstageLines(
          repoPath.value,
          filePath,
          op.hunkIndex,
          op.lineIndices ?? [],
        );
      }
      message.success("已取消暂存");
    }
    await loadDiff();
  } catch (e) {
    message.error("暂存操作失败: " + errMsg(e));
  } finally {
    stageLoading.value = false;
  }
}

// Untracked files have no patch hunks: stage them whole (git add).
async function stageWholeFile() {
  const file = selectedFile.value;
  if (!file || source.value !== "unstaged") return;
  stageLoading.value = true;
  try {
    const segments = repoPath.value.split(/[/\\]/);
    const name = segments.filter(Boolean).pop() ?? "repo";
    await batchAdd([
      { repoPath: repoPath.value, repoName: name, files: [file.newPath] },
    ]);
    message.success("已暂存整个文件");
    await loadDiff();
  } catch (e) {
    message.error("暂存失败: " + errMsg(e));
  } finally {
    stageLoading.value = false;
  }
}

/** Commit the staged area (index as-is), preserving hunk/line staging. */
async function handleStagedCommit() {
  const d = stagedCommitDialog.value;
  const commitMessage = d.message.trim();
  if (!commitMessage && !d.amend) {
    message.warning("请输入提交信息（Amend 可留空 = --no-edit）");
    return;
  }
  d.loading = true;
  try {
    const findings = await scanCommit(repoPath.value, [], true);
    if (findings.length > 0) {
      stagedScanDialog.value = { show: true, findings };
      return;
    }
    await submitStagedCommit(false);
  } catch (e) {
    message.error("安全检查失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}

async function handleStagedCommitOverride() {
  stagedScanDialog.value.show = false;
  await submitStagedCommit(true);
}

async function submitStagedCommit(allowUnsafe: boolean) {
  const d = stagedCommitDialog.value;
  const commitMessage = d.message.trim();
  d.loading = true;
  try {
    const segments = repoPath.value.split(/[/\\]/);
    const name = segments.filter(Boolean).pop() ?? "repo";
    await batchCommit([
      {
        repoPath: repoPath.value,
        repoName: name,
        message: commitMessage,
        files: [],
        amend: d.amend,
        noEdit: d.amend && !commitMessage,
        indexOnly: true,
        thenPush: d.thenPush,
        allowUnsafe,
      },
    ]);
    message.success("已提交 commit 任务");
    d.show = false;
    d.message = "";
    await loadDiff();
  } catch (e) {
    message.error("提交失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}

/**
 * AI-01：Provider/模型/凭证由 AI 设置解析（gitReview 任务默认链），
 * 不再前端传 Key。未配置/凭证缺失时引导打开 AI 设置（§12.4）。
 */
function applyAiCommitSuggestion(commitMessage: string) {
  stagedCommitDialog.value.message = commitMessage;
  stagedCommitDialog.value.show = true;
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
</script>

<style scoped>
.diff-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.diff-header {
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
}

.diff-options {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  white-space: nowrap;
}

.repo-path {
  font-size: 14px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
}

.diff-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.file-list {
  width: 280px;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
  background: var(--gw-bg-hover);
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
  font-family: var(--gw-font-mono);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.diff-content {
  flex: 1;
  overflow: auto;
  background: var(--gw-bg-panel);
}

.file-diff {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.file-diff-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 12px;
  background: var(--gw-bg-hover);
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
  font-weight: 500;
}

.file-diff-path {
  font-family: var(--gw-font-mono);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.compare-bar {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-hover);
}

.compare-sep {
  color: var(--gw-text-dim);
}

.staged-commit-options {
  display: flex;
  gap: var(--gw-space-4);
  margin-top: 12px;
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
  font-family: var(--gw-font-mono);
  color: var(--gw-text);
}

.scan-detail {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.file-diff-body {
  flex: 1;
  overflow: auto;
}

.review-result {
  padding: 12px;
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
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
}

.issue-desc {
  width: 100%;
  font-size: 13px;
  color: var(--gw-text);
  margin-top: 4px;
}
</style>

/* VirtualList owns diff-body scrolling now (T-04); clip the outer container
   so a double scrollbar can never appear. */
.file-diff-body {
  overflow: hidden;
}
