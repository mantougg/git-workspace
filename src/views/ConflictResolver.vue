<template>
  <div class="conflict-resolver">
    <!-- Header -->
    <div class="resolver-header">
      <span v-if="queueMode" class="repo-path">冲突队列（{{ workspaceName }}）</span>
      <template v-else>
        <span class="repo-path">{{ repoPath }}</span>
        <n-tag v-if="opLabel" size="small" type="warning">{{ opLabel }}</n-tag>
      </template>
      <n-button v-if="!queueMode" size="small" :loading="loading" @click="load">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
      <n-button v-if="!queueMode" size="small" :disabled="!repoPath" @click="openConflictAssistant">
        <template #icon><n-icon><SparklesOutline /></n-icon></template>
        AI 助手
      </n-button>
      <n-button size="small" disabled title="AI 冲突建议将在 T-26 提供">
        AI 建议（T-26）
      </n-button>
      <n-button
        v-if="queueMode && queue.length > 0"
        size="small"
        type="error"
        dashed
        @click="handleAbortAll"
      >
        全部中止
      </n-button>
    </div>

    <!-- Queue mode: all conflicted repos in the workspace -->
    <n-spin v-if="queueMode" :show="queueLoading">
      <div class="queue-body">
        <div v-for="q in queue" :key="q.repoPath" class="queue-row">
          <span class="queue-name">{{ q.repoName }}</span>
          <span class="queue-path">{{ q.repoPath }}</span>
          <n-tag size="small" type="error">{{ q.conflictCount }} 个冲突</n-tag>
          <n-tag size="small">{{ q.opLabel }}</n-tag>
          <n-button size="small" type="primary" dashed @click="openRepo(q.repoPath)">
            去解决
          </n-button>
        </div>
        <n-empty
          v-if="!queueLoading && queue.length === 0"
          description="工作区内没有冲突中的仓库"
        />
      </div>
    </n-spin>

    <!-- Single-repo mode -->
    <template v-else>
      <div class="resolver-progress">
        已解决 {{ resolvedCount }}/{{ conflicts.length }} 个文件
        <template v-if="state?.rebase">
          （Rebase 第 {{ state.rebase.position + 1 }}/{{ state.rebase.ops.length }} 步）
        </template>
      </div>
      <n-spin :show="loading">
        <div class="resolver-body">
          <!-- Conflict file list -->
          <div class="conflict-list">
            <div
              v-for="c in conflicts"
              :key="c.path"
              :class="['conflict-item', { active: selectedPath === c.path, resolved: isResolved(c.path) }]"
              @click="selectFile(c.path)"
            >
              <span class="conflict-path">{{ c.path }}</span>
              <n-tag size="small">{{ typeLabel(c.conflictType) }}</n-tag>
              <n-icon v-if="isResolved(c.path)" class="resolved-icon"><CheckmarkOutline /></n-icon>
            </div>
            <n-empty
              v-if="!loading && conflicts.length === 0"
              description="没有冲突文件"
            />
          </div>

          <!-- Three-way + result panes -->
          <div v-if="selectedPath" class="panes">
            <div class="pane-toolbar">
              <span class="pane-file">{{ selectedPath }}</span>
              <n-button size="small" @click="resolveWith('ours')">Use Ours</n-button>
              <n-button size="small" @click="resolveWith('theirs')">Use Theirs</n-button>
              <n-button size="small" @click="resolveWith('both')">Use Both</n-button>
              <n-button size="small" type="primary" :disabled="isResolved(selectedPath)" @click="resolveManual">
                应用 RESULT（Mark Resolved）
              </n-button>
            </div>
            <AiConflictAssistant
              v-if="content"
              :repo-path="repoPath"
              :path="selectedPath"
              :worktree="content.worktree ?? resultText"
              @candidate="useAiCandidate"
            />
            <div class="pane-grid">
              <div class="pane">
                <div class="pane-title">BASE</div>
                <pre class="pane-content">{{ content?.base ?? "（无共同祖先）" }}</pre>
              </div>
              <div class="pane">
                <div class="pane-title">OURS</div>
                <pre class="pane-content">{{ content?.ours ?? "（本侧已删除）" }}</pre>
              </div>
              <div class="pane">
                <div class="pane-title">THEIRS</div>
                <pre class="pane-content">{{ content?.theirs ?? "（对侧已删除）" }}</pre>
              </div>
              <div class="pane">
                <div class="pane-title">RESULT（可编辑）</div>
                <textarea v-model="resultText" class="pane-editor" spellcheck="false" />
              </div>
            </div>
            <div v-if="content?.truncated" class="truncate-hint">内容过大，部分侧已截断显示</div>
          </div>
          <n-empty v-else description="选择左侧文件开始解决" class="panes-empty" />
        </div>
      </n-spin>

      <!-- Footer: continue / abort -->
      <div class="resolver-footer">
        <n-button
          type="primary"
          :disabled="conflicts.length === 0 ? false : resolvedCount < conflicts.length"
          @click="handleContinue"
        >
          {{ continueLabel }}
        </n-button>
        <n-button type="error" dashed @click="handleAbort">中止（Abort）</n-button>
        <span v-if="conflicts.length > 0 && resolvedCount === conflicts.length" class="footer-hint">
          全部冲突已解决，可以继续
        </span>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
// @ts-nocheck — vue-tsc false positives: all bindings below ARE used in the template
import { computed, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { CheckmarkOutline, RefreshOutline, SparklesOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import {
  getConflictContent,
  getOperationState,
  resolveConflict,
  resolveConflictWithContent,
} from "@/api/conflict";
import { mergeAbort, mergeContinue } from "@/api/merge";
import { rebaseAbort, rebaseContinue } from "@/api/rebase";
import type { RebaseOutcome } from "@/types/rebase";
import { abortPick, pickContinue } from "@/api/history";
import { listRepositories } from "@/api/repository";
import type { ConflictContent, ConflictFile, OperationState } from "@/types/conflict";
import { errMsg } from "@/utils/error";
import AiConflictAssistant from "@/components/ai/AiConflictAssistant.vue";
import { useAiAssistant } from "@/composables/useAiAssistant";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();
const { openAssistant } = useAiAssistant();

const repoPath = ref("");
const queueMode = ref(false);
const workspaceName = ref("");
const workspaceId = ref<number | null>(null);
const queue = ref<Array<{ repoPath: string; repoName: string; conflictCount: number; opLabel: string }>>([]);
const queueLoading = ref(false);

const state = ref<OperationState | null>(null);
const loading = ref(false);
const selectedPath = ref("");
const content = ref<ConflictContent | null>(null);
const resultText = ref("");
/** True only after a complete AI candidate has entered the RESULT preview. */
const aiCandidatePending = ref(false);
/** Paths resolved during this session (no longer reported conflicted). */
const resolvedPaths = ref<Set<string>>(new Set());

const conflicts = computed<ConflictFile[]>(() => state.value?.conflicts ?? []);
const resolvedCount = computed(() => resolvedPaths.value.size);

const opLabel = computed(() => {
  const s = state.value;
  if (!s) return "";
  if (s.merge) return "Merge";
  if (s.rebase) return "Rebase";
  if (s.cherryPick) return "Cherry-pick";
  if (s.revert) return "Revert";
  return "冲突";
});

const continueLabel = computed(() => {
  if (state.value?.merge) return "继续 Merge（Continue）";
  if (state.value?.rebase) return "继续 Rebase（Continue）";
  if (state.value?.cherryPick) return "继续 Cherry-pick（Continue）";
  if (state.value?.revert) return "继续 Revert（Continue）";
  return "继续";
});

function openConflictAssistant() {
  if (!repoPath.value) return;
  const selected = selectedPath.value || "冲突概览";
  openAssistant({
    workspaceId: workspaceId.value,
    repositoryPaths: [repoPath.value],
    inferredRole: "conflictAssistant",
    origin: `Conflict Resolver · ${selected}`,
    supplementary: [{
      role: "userNote",
      kind: "diff",
      sourceId: `conflict:${repoPath.value}:${selected}`,
      displayName: `冲突范围：${selected}`,
      content: `当前操作：${opLabel.value || "冲突"}\n冲突文件：${conflicts.value.map((item) => item.path).join("\n")}`,
    }],
    draft: "请解释当前冲突的事实、可选解决策略及各自风险；不要修改文件。",
  });
}

onMounted(async () => {
  const ws = route.query.workspace as string | undefined;
  if (ws) {
    queueMode.value = true;
    workspaceId.value = Number(ws);
    workspaceName.value = (route.query.name as string) ?? "";
    await loadQueue();
    return;
  }
  const repo = route.query.repo as string;
  if (!repo) {
    message.warning("未指定仓库或工作区");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await load();
});

// ---------------------------------------------------------------------------
// Queue mode
// ---------------------------------------------------------------------------

async function loadQueue() {
  if (workspaceId.value === null) return;
  queueLoading.value = true;
  try {
    const repos = await listRepositories(workspaceId.value);
    const items: typeof queue.value = [];
    for (const r of repos) {
      try {
        const s = await getOperationState(r.repository.path);
        if (s.conflicts.length > 0 || s.merge || s.rebase || s.cherryPick || s.revert) {
          items.push({
            repoPath: r.repository.path,
            repoName: r.repository.name,
            conflictCount: s.conflicts.length,
            opLabel: s.merge ? "Merge" : s.rebase ? "Rebase" : s.cherryPick ? "Cherry-pick" : s.revert ? "Revert" : "冲突",
          });
        }
      } catch {
        // 单个仓库失败不阻塞队列
      }
    }
    queue.value = items;
  } catch (e) {
    message.error("加载冲突队列失败: " + errMsg(e));
  } finally {
    queueLoading.value = false;
  }
}

function openRepo(path: string) {
  router.push({ name: "conflict-resolver", query: { repo: path } });
  // Same-component navigation: switch modes manually.
  queueMode.value = false;
  repoPath.value = path;
  resolvedPaths.value = new Set();
  selectedPath.value = "";
  content.value = null;
  aiCandidatePending.value = false;
  load();
}

/** Abort every conflicted repo in the queue (Dangerous, §46). */
async function handleAbortAll() {
  const names = queue.value.map((q) => q.repoName).join("、");
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "整体 Abort 确认（Dangerous）",
        content: `将对以下 ${queue.value.length} 个仓库全部执行中止（Abort）：${names}\n\n每个仓库都会 hard reset 回操作前状态，冲突中的修改将丢失。`,
        positiveText: "全部中止",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject(new Error("cancelled")),
        onClose: () => reject(new Error("cancelled")),
      });
    });
  } catch {
    return;
  }
  let failed = 0;
  for (const q of queue.value) {
    try {
      const s = await getOperationState(q.repoPath);
      if (s.merge) await mergeAbort(q.repoPath);
      else if (s.rebase) await rebaseAbort(q.repoPath);
      else await abortPick(q.repoPath);
    } catch {
      failed += 1;
    }
  }
  if (failed > 0) {
    message.warning(`已中止 ${queue.value.length - failed} 个，${failed} 个失败`);
  } else {
    message.success(`已中止全部 ${queue.value.length} 个仓库的冲突操作`);
  }
  await loadQueue();
}

// ---------------------------------------------------------------------------
// Single-repo mode
// ---------------------------------------------------------------------------

async function load() {
  loading.value = true;
  try {
    state.value = await getOperationState(repoPath.value);
    // Keep the selection if the file is still conflicted.
    if (selectedPath.value && !conflicts.value.some((c) => c.path === selectedPath.value)) {
      selectedPath.value = "";
      content.value = null;
    }
    if (!selectedPath.value && conflicts.value.length > 0) {
      await selectFile(conflicts.value[0].path);
    }
  } catch (e) {
    message.error("加载冲突状态失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function isResolved(path: string): boolean {
  return resolvedPaths.value.has(path);
}

function typeLabel(t: string): string {
  switch (t) {
    case "both-modified": return "双方修改";
    case "both-added": return "双方新增";
    case "deleted-by-us": return "本侧删除";
    case "deleted-by-them": return "对侧删除";
    default: return t;
  }
}

async function selectFile(path: string) {
  selectedPath.value = path;
  content.value = null;
  try {
    content.value = await getConflictContent(repoPath.value, path);
    // RESULT starts from the worktree content (with markers) for manual edit.
    resultText.value =
      content.value.worktree ?? content.value.ours ?? content.value.theirs ?? "";
    aiCandidatePending.value = false;
  } catch (e) {
    message.error("加载冲突内容失败: " + errMsg(e));
  }
}

async function afterResolve(path: string) {
  resolvedPaths.value = new Set([...resolvedPaths.value, path]);
  aiCandidatePending.value = false;
  await load();
}

function useAiCandidate(candidate: string) {
  resultText.value = candidate;
  aiCandidatePending.value = true;
}

async function resolveWith(strategy: "ours" | "theirs" | "both") {
  const path = selectedPath.value;
  if (!path) return;
  try {
    await resolveConflict(repoPath.value, path, strategy);
    message.success(`已按 ${strategy} 解决 ${path}`);
    await afterResolve(path);
  } catch (e) {
    message.error("解决失败: " + errMsg(e));
  }
}

async function resolveManual() {
  const path = selectedPath.value;
  if (!path) return;
  if (aiCandidatePending.value) {
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.warning({
          title: "确认应用 AI 建议",
          content: "将把已预览的 AI 建议写入工作区并 Mark Resolved。此操作会修改该冲突文件；你仍可在继续操作前手动编辑或 Abort。",
          positiveText: "确认应用并标记已解决",
          negativeText: "取消",
          onPositiveClick: () => resolve(),
          onNegativeClick: () => reject(new Error("cancelled")),
          onClose: () => reject(new Error("cancelled")),
        });
      });
    } catch {
      return;
    }
  }
  try {
    await resolveConflictWithContent(repoPath.value, path, resultText.value);
    message.success(`已应用手动编辑：${path}`);
    await afterResolve(path);
  } catch (e) {
    message.error("解决失败: " + errMsg(e));
  }
}

async function handleContinue() {
  const s = state.value;
  if (!s) return;
  try {
    if (s.merge) {
      const oid = await mergeContinue(repoPath.value);
      message.success(`Merge 已完成（${oid.slice(0, 7)}）`);
    } else if (s.rebase) {
      const outcome: RebaseOutcome = await rebaseContinue(repoPath.value);
      if (outcome.status === "success") {
        message.success(`Rebase 完成（重写 ${outcome.rewritten} 个提交）`);
      } else {
        message.warning(
          `Rebase 第 ${outcome.position + 1}/${outcome.total} 步再次冲突，请继续解决`,
        );
      }
    } else if (s.cherryPick || s.revert) {
      const oid = await pickContinue(repoPath.value);
      message.success(`已继续并提交（${oid.slice(0, 7)}）`);
    }
    resolvedPaths.value = new Set();
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleAbort() {
  const s = state.value;
  if (!s) return;
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Abort 确认（Dangerous）",
        content: `仓库：${repoPath.value}\n将放弃当前 ${opLabel.value} 操作并恢复到操作前状态（hard reset），冲突中的修改将丢失。`,
        positiveText: "中止并恢复",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject(new Error("cancelled")),
        onClose: () => reject(new Error("cancelled")),
      });
    });
  } catch {
    return;
  }
  try {
    if (s.merge) await mergeAbort(repoPath.value);
    else if (s.rebase) await rebaseAbort(repoPath.value);
    else await abortPick(repoPath.value);
    message.success("已中止并恢复");
    resolvedPaths.value = new Set();
    await load();
  } catch (e) {
    message.error("Abort 失败: " + errMsg(e));
  }
}

</script>

<style scoped>
.conflict-resolver {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.resolver-header {
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
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.resolver-progress {
  padding: 6px 16px;
  font-size: 13px;
  color: var(--gw-text-dim);
  background: var(--gw-bg-hover);
  border-bottom: 1px solid var(--gw-border);
}

.resolver-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.conflict-list {
  width: 300px;
  border-right: 1px solid var(--gw-border);
  overflow-y: auto;
  background: var(--gw-bg-hover);
}

.conflict-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 8px 12px;
  cursor: pointer;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.conflict-item:hover {
  background: var(--gw-bg-hover);
}

.conflict-item.active {
  background: var(--gw-bg-hover);
}

.conflict-item.resolved .conflict-path {
  color: var(--gw-success);
  text-decoration: line-through;
}

.conflict-path {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--gw-font-mono);
  font-size: 12px;
}

.resolved-icon {
  color: var(--gw-success);
}

.panes {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panes-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}

.pane-toolbar {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 12px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.pane-file {
  flex: 1;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pane-grid {
  flex: 1;
  display: grid;
  grid-template-columns: 1fr 1fr;
  grid-template-rows: 1fr 1fr;
  gap: 1px;
  background: var(--gw-border);
  overflow: hidden;
}

.pane {
  display: flex;
  flex-direction: column;
  background: var(--gw-bg-panel);
  overflow: hidden;
}

.pane-title {
  padding: 4px 10px;
  font-size: 12px;
  font-weight: 600;
  color: var(--gw-text-dim);
  background: var(--gw-bg-hover);
  border-bottom: 1px solid var(--gw-border);
}

.pane-content {
  flex: 1;
  overflow: auto;
  margin: 0;
  padding: 8px 10px;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  white-space: pre;
}

.pane-editor {
  flex: 1;
  border: none;
  outline: none;
  resize: none;
  padding: 8px 10px;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  background: var(--gw-bg-panel);
}

.truncate-hint {
  padding: 4px 12px;
  font-size: 12px;
  color: var(--gw-warning);
  background: var(--gw-warning);
}

.resolver-footer {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-top: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.footer-hint {
  color: var(--gw-success);
  font-size: 13px;
}

.queue-body {
  flex: 1;
  overflow-y: auto;
  background: var(--gw-bg-panel);
}

.queue-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 10px 16px;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.queue-name {
  font-weight: 600;
  width: 160px;
  flex-shrink: 0;
}

.queue-path {
  flex: 1;
  color: var(--gw-text-dim);
  font-family: var(--gw-font-mono);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
