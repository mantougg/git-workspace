<template>
  <div class="conflict-assistant">
    <div class="assistant-toolbar">
      <span class="assistant-progress">AI 建议 {{ proposals.size }}/{{ hunks.length }} 段</span>
      <n-button
        size="small"
        type="primary"
        :loading="building"
        :disabled="!nextHunk || processing"
        @click="() => buildPreview()"
      >
        {{ proposals.size === 0 ? "生成 AI 建议" : "生成下一段建议" }}
      </n-button>
      <n-button v-if="processing" size="small" @click="cancel">取消当前请求</n-button>
      <n-button
        v-if="readyCandidate"
        size="small"
        type="primary"
        @click="useCandidate"
      >
        写入 RESULT 预览
      </n-button>
    </div>

    <div v-if="proposals.size > 0" class="suggestions">
      <div v-for="hunk in hunks" :key="hunk.index" class="suggestion-row">
        <span>Hunk {{ hunk.index + 1 }}/{{ hunks.length }}</span>
        <template v-if="proposals.get(hunk.index)">
          <n-tag size="small" :type="confidenceType(proposals.get(hunk.index)!.confidence)">
            {{ confidenceLabel(proposals.get(hunk.index)!.confidence) }}
          </n-tag>
          <span class="rationale">{{ proposals.get(hunk.index)!.rationale }}</span>
          <n-button size="tiny" quaternary @click="showPreview(hunk.index)">查看 Diff</n-button>
        </template>
        <span v-else class="pending">等待建议</span>
      </div>
    </div>

    <n-modal v-model:show="diffVisible" preset="card" title="AI 冲突建议 Diff Preview" class="diff-modal">
      <template v-if="activeProposal">
        <div class="proposal-meta">
          <n-tag size="small" :type="confidenceType(activeProposal.confidence)">
            {{ confidenceLabel(activeProposal.confidence) }}
          </n-tag>
          <span>{{ activeProposal.rationale }}</span>
        </div>
        <pre class="diff-content">{{ activeProposal.diff }}</pre>
      </template>
    </n-modal>

    <AiRequestPreview
      v-model="previewVisible"
      :preview="preview"
      :loading="building"
      :confirming="confirming"
      @confirm="confirm"
      @toggle-exclusion="toggleExclusion"
      @confirm-warn="confirmWarn"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useMessage } from "naive-ui";
import AiRequestPreview from "@/components/ai/AiRequestPreview.vue";
import {
  aiApproveRequest,
  aiBuildContextPreview,
  aiCancelRequest,
  aiGetRequestStatus,
  aiSubmitRequest,
} from "@/api/ai";
import type {
  AiContextPreview,
  AiRequestSnapshot,
  AiResult,
  ConflictProposal,
  ContextPreviewRequest,
} from "@/types/ai";
import { errMsg } from "@/utils/error";

interface ConflictHunk {
  index: number;
  start: number;
  end: number;
}

const props = defineProps<{
  repoPath: string;
  path: string;
  worktree: string;
}>();

const emit = defineEmits<{
  /** A complete candidate remains local to the RESULT editor until T-16 confirmation. */
  candidate: [content: string];
}>();

const message = useMessage();
const building = ref(false);
const confirming = ref(false);
const processing = ref(false);
const previewVisible = ref(false);
const preview = ref<AiContextPreview | null>(null);
const previewRequest = ref<ContextPreviewRequest | null>(null);
const snapshot = ref<AiRequestSnapshot | null>(null);
const activeHunk = ref<number | null>(null);
const proposals = ref(new Map<number, ConflictProposal>());
const diffVisible = ref(false);
const diffHunk = ref<number | null>(null);
let pollTimer: ReturnType<typeof setTimeout> | null = null;

const hunks = computed(() => splitConflictHunks(props.worktree));
const nextHunk = computed(() => hunks.value.find((hunk) => !proposals.value.has(hunk.index)) ?? null);
const readyCandidate = computed(() => hunks.value.length > 0 && proposals.value.size === hunks.value.length);
const activeProposal = computed(() => diffHunk.value === null ? null : proposals.value.get(diffHunk.value) ?? null);

watch(() => [props.repoPath, props.path, props.worktree], () => reset());

function reset() {
  clearPoll();
  previewVisible.value = false;
  preview.value = null;
  previewRequest.value = null;
  snapshot.value = null;
  activeHunk.value = null;
  proposals.value = new Map();
  processing.value = false;
}

function makeRequest(hunk: ConflictHunk): ContextPreviewRequest {
  return {
    taskKind: "conflict",
    gitScenario: null,
    providerId: null,
    modelId: null,
    workspaceId: null,
    repoPath: props.repoPath,
    conflict: { path: props.path, hunkIndex: hunk.index, hunkTotal: hunks.value.length },
    runtimeName: null,
    processId: null,
    project: null,
    userInstruction: `仅解决 ${props.path} 的第 ${hunk.index + 1}/${hunks.value.length} 个冲突 hunk。proposedContent 必须只包含该 hunk 的替换文本，不包含 Git 冲突标记或 Markdown。diff 必须描述该 hunk 相对 WORKTREE 的变更。`,
    diffScope: null,
    diffSelection: null,
    supplementary: [],
    exclusions: [],
    secretPolicy: { strategy: "block", warnConfirmed: false },
    budgetStrategy: "codeReview",
    stream: false,
    tokenEstimateFactor: null,
    logTailLines: null,
    tokenBudget: null,
    includeRuntimeLogs: false,
  };
}

async function buildPreview(request?: ContextPreviewRequest) {
  const hunk = request ? null : nextHunk.value;
  if (!request && !hunk) return;
  building.value = true;
  try {
    const next = await aiBuildContextPreview(request ?? makeRequest(hunk!));
    preview.value = next;
    previewRequest.value = request ?? makeRequest(hunk!);
    activeHunk.value = previewRequest.value?.conflict?.hunkIndex ?? hunk?.index ?? null;
    snapshot.value = null;
    previewVisible.value = true;
  } catch (error) {
    message.error("AI 冲突预览失败: " + errMsg(error));
  } finally {
    building.value = false;
  }
}

async function toggleExclusion(sourceId: string, included: boolean) {
  const current = previewRequest.value;
  if (!current || building.value || confirming.value) return;
  const exclusions = new Set(current.exclusions ?? []);
  if (included) exclusions.delete(sourceId);
  else exclusions.add(sourceId);
  await buildPreview({ ...current, exclusions: [...exclusions] });
}

async function confirmWarn() {
  const current = previewRequest.value;
  if (!current || current.secretPolicy?.strategy !== "warn") return;
  await buildPreview({
    ...current,
    secretPolicy: { ...current.secretPolicy, warnConfirmed: true },
  });
}

async function confirm() {
  if (!preview.value || preview.value.blocked || confirming.value) return;
  confirming.value = true;
  try {
    const submitted = await aiSubmitRequest({ ...preview.value.request, useCache: true });
    snapshot.value = submitted;
    const approved = submitted.phase === "previewRequired"
      ? await aiApproveRequest(submitted.requestId)
      : submitted;
    snapshot.value = approved;
    processing.value = !isTerminal(approved.phase);
    schedulePoll(approved.requestId);
  } catch (error) {
    message.error("AI 请求失败: " + errMsg(error));
  } finally {
    confirming.value = false;
  }
}

function schedulePoll(requestId: string) {
  clearPoll();
  pollTimer = setTimeout(() => void poll(requestId), 350);
}

function clearPoll() {
  if (pollTimer) clearTimeout(pollTimer);
  pollTimer = null;
}

async function poll(requestId: string) {
  try {
    const next = await aiGetRequestStatus(requestId);
    if (!next) return;
    snapshot.value = next;
    if (!isTerminal(next.phase)) {
      schedulePoll(requestId);
      return;
    }
    processing.value = false;
    clearPoll();
    if (next.phase === "succeeded" && next.result) {
      acceptResult(next.result);
    } else if (next.phase !== "cancelled") {
      message.error(next.error ?? "AI 请求未完成");
    }
  } catch (error) {
    processing.value = false;
    message.error("AI 状态查询失败: " + errMsg(error));
  }
}

function acceptResult(result: AiResult) {
  if (result.type !== "conflictProposal" || activeHunk.value === null) {
    message.error("AI 返回了不完整的冲突建议，未写入 RESULT");
    return;
  }
  if (containsConflictMarker(result.payload.proposedContent)) {
    message.error("AI 建议仍含冲突标记，未写入 RESULT");
    return;
  }
  proposals.value = new Map(proposals.value).set(activeHunk.value, result.payload);
  previewVisible.value = false;
  message.success(`已生成 Hunk ${activeHunk.value + 1}/${hunks.value.length} 的建议`);
}

async function cancel() {
  const requestId = snapshot.value?.requestId;
  if (!requestId) return;
  try {
    await aiCancelRequest(requestId);
    processing.value = false;
    clearPoll();
    message.info("已取消当前 AI 请求；已有建议会保留");
  } catch (error) {
    message.error("取消失败: " + errMsg(error));
  }
}

function useCandidate() {
  if (!readyCandidate.value) return;
  emit("candidate", buildCandidate());
  message.info("AI 建议已写入 RESULT 预览，确认应用前不会修改工作区");
}

function buildCandidate(): string {
  let output = "";
  let offset = 0;
  for (const hunk of hunks.value) {
    output += props.worktree.slice(offset, hunk.start);
    output += proposals.value.get(hunk.index)?.proposedContent ?? props.worktree.slice(hunk.start, hunk.end);
    offset = hunk.end;
  }
  return output + props.worktree.slice(offset);
}

function showPreview(index: number) {
  diffHunk.value = index;
  diffVisible.value = true;
}

function confidenceType(value: ConflictProposal["confidence"]): "success" | "warning" | "default" {
  return value === "high" ? "success" : value === "medium" ? "warning" : "default";
}

function confidenceLabel(value: ConflictProposal["confidence"]): string {
  return value === "high" ? "高置信度" : value === "medium" ? "中置信度" : "低置信度";
}

function isTerminal(phase: string): boolean {
  return ["succeeded", "cancelled", "rejected", "failed", "degraded"].includes(phase);
}

function containsConflictMarker(value: string): boolean {
  return /^(<<<<<<<|=======|>>>>>>>)/m.test(value);
}

function splitConflictHunks(value: string): ConflictHunk[] {
  const hunks: ConflictHunk[] = [];
  let cursor = 0;
  while (cursor < value.length) {
    const start = value.indexOf("<<<<<<<", cursor);
    if (start < 0) break;
    const endMarker = value.indexOf(">>>>>>>", start);
    if (endMarker < 0) break;
    const endLine = value.indexOf("\n", endMarker);
    const end = endLine < 0 ? value.length : endLine + 1;
    hunks.push({ index: hunks.length, start, end });
    cursor = end;
  }
  return hunks.length > 0 ? hunks : [{ index: 0, start: 0, end: value.length }];
}

onBeforeUnmount(() => {
  clearPoll();
});
</script>

<style scoped>
.conflict-assistant,
.suggestions {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.assistant-toolbar,
.suggestion-row,
.proposal-meta {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.assistant-toolbar {
  flex-wrap: wrap;
}

.assistant-progress,
.pending {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.suggestion-row {
  min-width: 0;
  font-size: var(--gw-text-sm);
}

.rationale {
  min-width: 0;
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.diff-modal {
  width: min(760px, calc(100vw - var(--gw-space-4) * 2));
}

.proposal-meta {
  align-items: flex-start;
  margin-bottom: var(--gw-space-3);
  font-size: var(--gw-text-sm);
}

.diff-content {
  max-height: 48vh;
  margin: 0;
  overflow: auto;
  padding: var(--gw-space-3);
  border: 1px solid var(--gw-border);
  background: var(--gw-bg-app);
  color: var(--gw-text);
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-xs);
  white-space: pre-wrap;
}
</style>
