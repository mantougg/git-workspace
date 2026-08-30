<template>
  <n-modal v-model:show="pickerVisible" preset="card" title="Git AI Assistant" class="git-assistant-modal">
    <div class="assistant-form">
      <n-select v-model:value="scenario" :options="scenarioOptions" />
      <AiDiffSelection v-model="selection" :repositories="repositories" />
    </div>
    <template #footer>
      <n-button @click="pickerVisible = false">取消</n-button>
      <n-button
        type="primary"
        :disabled="selection.repositories.length === 0 || building"
        :loading="building"
        @click="() => buildPreview()"
      >
        生成发送预览
      </n-button>
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

  <n-modal v-model:show="resultVisible" preset="card" :title="resultTitle" class="git-assistant-modal">
    <div v-if="result" class="result-body">
      <template v-if="result.type === 'commitSuggestion'">
        <n-input v-model:value="editableCommitTitle" placeholder="Commit title" />
        <n-input v-model:value="editableCommitBody" type="textarea" :rows="5" placeholder="Commit body" />
        <span class="result-note">{{ stringValue(result.payload.rationale) }}</span>
      </template>
      <template v-else-if="result.type === 'reviewReport'">
        <div class="result-summary">{{ stringValue(result.payload.summary) }}</div>
        <div v-for="(issue, index) in reviewIssues" :key="index" class="issue-row">
          <n-tag size="small" :type="severityType(issue.severity)">{{ issue.severity }}</n-tag>
          <n-tag size="small" :bordered="false">{{ issue.category }}</n-tag>
          <span class="mono">{{ issue.file }}<template v-if="issue.line != null">:{{ issue.line }}</template></span>
          <span>{{ issue.description }}</span>
        </div>
        <n-empty v-if="reviewIssues.length === 0" description="未发现可确认的问题" />
      </template>
      <template v-else>
        <div class="result-summary">{{ resultSummary }}</div>
        <n-input v-if="editableText" v-model:value="editableText" type="textarea" :rows="12" />
      </template>
    </div>
    <template #footer>
      <n-button @click="copyResult">复制</n-button>
      <n-button
        v-if="result?.type === 'commitSuggestion'"
        type="primary"
        @click="applyCommitSuggestion"
      >
        填入提交信息
      </n-button>
      <n-button v-else type="primary" @click="resultVisible = false">完成</n-button>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useMessage } from "naive-ui";
import AiDiffSelection from "@/components/ai/AiDiffSelection.vue";
import AiRequestPreview from "@/components/ai/AiRequestPreview.vue";
import {
  aiApproveRequest,
  aiBuildContextPreview,
  aiGetRequestStatus,
  aiSubmitRequest,
} from "@/api/ai";
import type {
  AiContextPreview,
  AiRequestSnapshot,
  AiResult,
  ContextPreviewRequest,
  DiffRepositorySelection,
  GitAssistantScenario,
  GitDiffSelection,
  SupplementaryContext,
} from "@/types/ai";
import { errMsg } from "@/utils/error";

interface RepositoryOption {
  repoPath: string;
  name: string;
  files: string[];
}

const props = withDefaults(defineProps<{
  workspaceId?: number | null;
  repositories: RepositoryOption[];
  source?: "workdir" | "staged" | "unstaged";
  initialScenario?: GitAssistantScenario;
  supplementary?: SupplementaryContext[];
}>(), {
  workspaceId: null,
  source: "workdir",
  initialScenario: "codeReview",
  supplementary: () => [],
});

const emit = defineEmits<{
  applyCommitSuggestion: [message: string];
}>();

const pickerVisible = defineModel<boolean>({ default: false });
const message = useMessage();
const scenario = ref<GitAssistantScenario>(props.initialScenario);
const selection = ref<GitDiffSelection>({ repositories: [] });
const building = ref(false);
const confirming = ref(false);
const previewVisible = ref(false);
const resultVisible = ref(false);
const preview = ref<AiContextPreview | null>(null);
const previewRequest = ref<ContextPreviewRequest | null>(null);
const snapshot = ref<AiRequestSnapshot | null>(null);
const result = ref<AiResult | null>(null);
const editableCommitTitle = ref("");
const editableCommitBody = ref("");
const editableText = ref("");
let pollTimer: ReturnType<typeof setTimeout> | null = null;

const scenarioOptions = [
  ["commitMessage", "Commit Message"],
  ["commitSummary", "Commit Summary"],
  ["codeReview", "Code Review"],
  ["securityReview", "Security Review"],
  ["bugDetection", "Bug Detection"],
  ["prDescription", "PR Description"],
  ["commitExplanation", "Commit Explanation"],
  ["fileExplanation", "File Explanation"],
].map(([value, label]) => ({ value, label }));

watch(pickerVisible, (visible) => {
  if (!visible) return;
  scenario.value = props.initialScenario;
  selection.value = {
    repositories: props.repositories.map<DiffRepositorySelection>((repo) => ({
      repoPath: repo.repoPath,
      includePaths: [],
      excludePaths: [],
    })),
  };
});

function taskKindFor(value: GitAssistantScenario): "gitReview" | "commitMessage" {
  return value === "commitMessage" || value === "commitSummary" ? "commitMessage" : "gitReview";
}

function budgetFor(value: GitAssistantScenario): ContextPreviewRequest["budgetStrategy"] {
  if (value === "commitMessage") return "commitMessage";
  if (value === "commitSummary" || value === "prDescription") return "multiRepoSummary";
  return "codeReview";
}

function makeRequest(): ContextPreviewRequest | null {
  const repoPath = selection.value.repositories[0]?.repoPath;
  if (!repoPath) return null;
  return {
    taskKind: taskKindFor(scenario.value),
    gitScenario: scenario.value,
    providerId: null,
    modelId: null,
    workspaceId: props.workspaceId,
    repoPath,
    runtimeName: null,
    processId: null,
    project: null,
    userInstruction: "",
    diffScope: props.source,
    diffSelection: selection.value,
    supplementary: props.supplementary,
    exclusions: [],
    secretPolicy: { strategy: "block", warnConfirmed: false },
    budgetStrategy: budgetFor(scenario.value),
    stream: false,
    tokenEstimateFactor: null,
    logTailLines: null,
    tokenBudget: null,
    includeRuntimeLogs: false,
  };
}

async function buildPreview(request = makeRequest()) {
  if (!request) return;
  building.value = true;
  try {
    const next = await aiBuildContextPreview({ ...request, exclusions: [...(request.exclusions ?? [])] });
    preview.value = next;
    previewRequest.value = request;
    snapshot.value = null;
    result.value = null;
    pickerVisible.value = false;
    previewVisible.value = true;
  } catch (error) {
    message.error("AI 预览失败: " + errMsg(error));
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

function clearPoll() {
  if (pollTimer) clearTimeout(pollTimer);
  pollTimer = null;
}

function schedulePoll(requestId: string) {
  clearPoll();
  pollTimer = setTimeout(() => void poll(requestId), 350);
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
    schedulePoll(approved.requestId);
  } catch (error) {
    message.error("AI 请求失败: " + errMsg(error));
  } finally {
    confirming.value = false;
  }
}

async function poll(requestId: string) {
  try {
    const next = await aiGetRequestStatus(requestId);
    if (!next) return;
    snapshot.value = next;
    if (![
      "succeeded", "cancelled", "rejected", "failed", "degraded",
    ].includes(next.phase)) {
      schedulePoll(requestId);
      return;
    }
    clearPoll();
    if (next.phase === "succeeded" && next.result) {
      openResult(next.result);
    } else if (next.error) {
      message.error("AI 请求未完成: " + next.error);
    }
  } catch (error) {
    message.error("AI 状态查询失败: " + errMsg(error));
  }
}

function openResult(next: AiResult) {
  result.value = next;
  if (next.type === "commitSuggestion") {
    editableCommitTitle.value = stringValue(next.payload.title);
    editableCommitBody.value = stringArray(next.payload.body).join("\n");
  } else {
    editableText.value = printable(next);
  }
  previewVisible.value = false;
  resultVisible.value = true;
}

function stringValue(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
}

const reviewIssues = computed(() => {
  if (result.value?.type !== "reviewReport" || !Array.isArray(result.value.payload.issues)) return [];
  return result.value.payload.issues.flatMap((raw) => {
    if (!raw || typeof raw !== "object") return [];
    const issue = raw as Record<string, unknown>;
    if (!["severity", "category", "file", "description"].every((key) => typeof issue[key] === "string")) return [];
    return [{
      severity: issue.severity as string,
      category: issue.category as string,
      file: issue.file as string,
      line: typeof issue.line === "number" ? issue.line : null,
      description: issue.description as string,
    }];
  });
});

const resultTitle = computed(() => scenarioOptions.find((item) => item.value === scenario.value)?.label ?? "AI 结果");
const resultSummary = computed(() => result.value ? printable(result.value) : "");

function printable(value: AiResult): string {
  if (value.type === "answer" || value.type === "generatedText") return value.text;
  if (value.type === "reviewReport") return stringValue(value.payload.summary);
  return JSON.stringify(value.payload, null, 2);
}

function severityType(value: string): "error" | "warning" | "info" {
  return value === "high" ? "error" : value === "medium" ? "warning" : "info";
}

async function copyResult() {
  if (!result.value) return;
  try {
    await navigator.clipboard.writeText(
      result.value.type === "commitSuggestion"
        ? [editableCommitTitle.value, editableCommitBody.value].filter(Boolean).join("\n\n")
        : editableText.value || printable(result.value),
    );
    message.success("AI 结果已复制");
  } catch {
    message.error("复制失败，请检查剪贴板权限");
  }
}

function applyCommitSuggestion() {
  const text = [editableCommitTitle.value.trim(), editableCommitBody.value.trim()].filter(Boolean).join("\n\n");
  if (!text) return;
  emit("applyCommitSuggestion", text);
  resultVisible.value = false;
}
</script>

<style scoped>
.assistant-form,
.result-body {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.git-assistant-modal {
  width: min(70vw, calc(100vw - var(--gw-space-4) * 2));
}

.result-summary {
  white-space: pre-wrap;
}

.result-note {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.issue-row {
  display: grid;
  grid-template-columns: auto auto max-content 1fr;
  align-items: start;
  gap: var(--gw-space-2);
  font-size: var(--gw-text-sm);
}

.mono {
  font-family: var(--gw-font-mono);
}
</style>
