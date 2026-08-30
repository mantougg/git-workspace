<template>
  <n-card size="small" class="suggestion-card" :bordered="true">
    <template #header>
      <div class="card-header">
        <span>Runtime 诊断结果</span>
        <n-tag v-if="report?.confidence" size="small" :type="confidenceType" :bordered="false">
          置信度 {{ confidenceLabel }}
        </n-tag>
      </div>
    </template>

    <div v-if="report" class="report-body">
      <h3 class="headline">{{ report.headline || "暂无结论" }}</h3>

      <section v-if="report.facts.length" class="report-section facts-section">
        <div class="section-title">确定性事实</div>
        <ul class="report-list">
          <li v-for="(fact, index) in report.facts" :key="`fact-${index}`">{{ fact }}</li>
        </ul>
      </section>

      <section v-if="report.likelyCauses.length" class="report-section advice-section">
        <div class="section-title">
          <n-tag size="small" type="warning" :bordered="false">AI 建议</n-tag>
          可能原因
        </div>
        <ul class="report-list">
          <li v-for="(cause, index) in report.likelyCauses" :key="`cause-${index}`">{{ cause }}</li>
        </ul>
      </section>

      <section v-if="report.suggestedActions.length" class="report-section advice-section">
        <div class="section-title">
          <n-tag size="small" type="warning" :bordered="false">待确认</n-tag>
          排查与处理建议
        </div>
        <ol class="report-list">
          <li v-for="(action, index) in report.suggestedActions" :key="`action-${index}`">{{ action }}</li>
        </ol>
      </section>

      <section v-if="report.needsUserCheck.length" class="report-section">
        <div class="section-title">需要确认</div>
        <ul class="report-list">
          <li v-for="(item, index) in report.needsUserCheck" :key="`check-${index}`">{{ item }}</li>
        </ul>
      </section>

      <n-collapse v-if="sources.length" class="source-collapse">
        <n-collapse-item title="查看上下文来源" name="sources">
          <div class="source-list">
            <div v-for="source in sources" :key="source.sourceId" class="source-row">
              <span>{{ source.displayName }}</span>
              <span class="source-meta mono">
                {{ source.charCount }} 字 · {{ source.estimatedTokens }} tok
                <template v-if="source.excluded"> · 已排除</template>
              </span>
            </div>
          </div>
        </n-collapse-item>
      </n-collapse>
    </div>

    <div v-else-if="answer" class="answer mono">{{ answer }}</div>

    <template #footer>
      <div class="card-footer">
        <span v-if="runtimeName" class="scope mono">
          {{ runtimeName }}<template v-if="processId != null"> · 进程 #{{ processId }}</template>
        </span>
        <span class="footer-actions">
          <n-button size="small" @click="copyResult">复制</n-button>
          <n-button size="small" type="primary" @click="emit('retry')">重新分析</n-button>
        </span>
      </div>
    </template>
  </n-card>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useMessage } from "naive-ui";
import type { AiRequestSnapshot, AiResult, ContextItem, DiagnosticReport } from "@/types/ai";

const props = defineProps<{
  snapshot: AiRequestSnapshot | null;
  sources: ContextItem[];
  runtimeName?: string | null;
  processId?: number | null;
}>();

const emit = defineEmits<{ retry: [] }>();
const message = useMessage();

function asStrings(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => (typeof item === "string" ? item : JSON.stringify(item))).filter(Boolean) as string[];
}

function parseReport(result: AiResult | null): DiagnosticReport | null {
  if (!result || result.type !== "diagnosticReport") return null;
  const payload = result.payload;
  return {
    headline: typeof payload.headline === "string" ? payload.headline : "",
    confidence: payload.confidence === "high" || payload.confidence === "medium" || payload.confidence === "low"
      ? payload.confidence
      : "low",
    facts: asStrings(payload.facts),
    likelyCauses: asStrings(payload.likelyCauses),
    suggestedActions: asStrings(payload.suggestedActions),
    needsUserCheck: asStrings(payload.needsUserCheck),
    sourceContext: asStrings(payload.sourceContext),
  };
}

const report = computed(() => parseReport(props.snapshot?.result ?? null));
const answer = computed(() => {
  const result = props.snapshot?.result;
  return result?.type === "answer" ? result.text : null;
});
const confidenceLabel = computed(() => report.value?.confidence ?? "low");
const confidenceType = computed(() => {
  switch (report.value?.confidence) {
    case "high": return "success";
    case "medium": return "warning";
    default: return "default";
  }
});

const copyText = computed(() => {
  if (report.value) {
    return [
      report.value.headline,
      "确定性事实：",
      ...report.value.facts.map((item) => `- ${item}`),
      "可能原因（AI 建议）：",
      ...report.value.likelyCauses.map((item) => `- ${item}`),
      "排查与处理建议（待确认）：",
      ...report.value.suggestedActions.map((item) => `- ${item}`),
      "需要确认：",
      ...report.value.needsUserCheck.map((item) => `- ${item}`),
    ].filter(Boolean).join("\n");
  }
  return answer.value ?? "";
});

async function copyResult() {
  if (!copyText.value) return;
  try {
    await navigator.clipboard.writeText(copyText.value);
    message.success("诊断结果已复制");
  } catch {
    message.error("复制诊断结果失败，请检查剪贴板权限");
  }
}
</script>

<style scoped>
.suggestion-card {
  max-width: 100%;
}
.card-header,
.card-footer,
.footer-actions,
.source-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}
.card-header,
.card-footer {
  justify-content: space-between;
}
.report-body {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}
.headline {
  margin: 0;
  font-size: var(--gw-text-md);
  font-weight: 600;
}
.report-section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}
.section-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
  font-weight: 600;
}
.facts-section .section-title {
  color: var(--gw-success);
}
.report-list {
  margin: 0;
  padding-left: var(--gw-space-4);
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  line-height: 1.5;
  white-space: pre-wrap;
}
.source-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}
.source-row {
  justify-content: space-between;
  min-width: 0;
  font-size: var(--gw-text-sm);
}
.source-row > span:first-child {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.source-meta,
.scope,
.answer {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-xs);
}
.answer {
  white-space: pre-wrap;
  line-height: 1.5;
}
.mono {
  font-family: var(--gw-font-mono);
}
@media (max-width: 640px) {
  .card-footer,
  .source-row {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
