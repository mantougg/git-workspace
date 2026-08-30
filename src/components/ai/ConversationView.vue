<template>
  <div ref="scrollRef" class="conversation">
    <n-button
      v-if="canLoadEarlier"
      size="tiny"
      quaternary
      class="load-earlier"
      @click="emit('load-earlier')"
    >
      加载更早消息
    </n-button>
    <n-empty
      v-if="messages.length === 0 && !streamingText && toolReads.length === 0"
      class="conversation-empty"
      description="向 GitWorkspace Assistant 提问，或使用下方工具读取应用状态"
    />

    <div v-for="message in messages" :key="message.id" class="message-row" :class="message.role">
      <div class="message-bubble">
        <div class="message-meta">
          {{ message.role === "user" ? "你" : "助手" }} · {{ formatTime(message.createdAt) }}
        </div>
        <div class="message-text">{{ textOf(message) }}</div>
        <n-collapse v-if="hasDetails(message)" class="message-details">
          <n-collapse-item title="查看结构化结果" name="details">
            <pre class="details-pre">{{ detailsOf(message) }}</pre>
          </n-collapse-item>
        </n-collapse>
      </div>
    </div>

    <!-- 流式输出（§16.1 合帧渲染，store 侧 rAF 合并） -->
    <div v-if="streamingText" class="message-row assistant">
      <div class="message-bubble">
        <div class="message-meta">助手 · 正在生成…</div>
        <div class="message-text">{{ streamingText }}</div>
      </div>
    </div>

    <!-- 工具读取摘要（§12.3 中部；§9.3 只读工具） -->
    <div v-for="card in toolReads" :key="card.id" class="tool-card">
      <div class="tool-card-header">
        <n-tag size="small" :bordered="false" type="info">工具</n-tag>
        <span class="tool-name mono">{{ card.toolName }}</span>
        <span class="tool-meta">{{ card.durationMs }} ms</span>
        <n-tag v-if="card.truncated" size="small" :bordered="false" type="warning">已截断</n-tag>
      </div>
      <div class="tool-args mono">参数 {{ card.argsSummary }}</div>
      <div v-if="card.error" class="tool-error">调用失败：{{ card.error }}</div>
      <template v-else>
        <div class="tool-result">{{ card.resultSummary }}<template v-if="card.resultJson.length > card.resultSummary.length">…</template></div>
        <n-collapse class="message-details">
          <n-collapse-item title="展开查看来源" name="source">
            <pre class="details-pre">{{ card.resultJson }}</pre>
          </n-collapse-item>
        </n-collapse>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import type { AiSessionMessage } from "@/types/ai";
import type { ToolReadCard } from "@/stores/ai";

const props = defineProps<{
  messages: AiSessionMessage[];
  canLoadEarlier: boolean;
  streamingText: string;
  toolReads: ToolReadCard[];
}>();

const emit = defineEmits<{ "load-earlier": [] }>();

const scrollRef = ref<HTMLElement | null>(null);

watch(
  () => [props.messages.length, props.streamingText.length, props.toolReads.length],
  async () => {
    await nextTick();
    scrollRef.value?.scrollTo({ top: scrollRef.value.scrollHeight });
  },
);

/** 用户消息取最后的指令（前部是当轮上下文消息，不重复展示）。 */
function textOf(message: AiSessionMessage): string {
  const content = message.content as Record<string, unknown> | null;
  if (!content) return "";
  if (message.role === "user") {
    const arr = content.messages as Array<{ content?: unknown }> | undefined;
    const last = arr?.[arr.length - 1];
    return typeof last?.content === "string" ? last.content : "";
  }
  if (typeof content.text === "string") return content.text;
  const payload = content.payload as Record<string, unknown> | undefined;
  const parts: string[] = [];
  for (const key of ["headline", "summary", "title"]) {
    const value = payload?.[key];
    if (typeof value === "string" && value) parts.push(value);
  }
  if (parts.length > 0) return parts.join("\n");
  for (const key of ["facts", "likelyCauses", "suggestedActions", "details", "risks"]) {
    const value = payload?.[key];
    if (Array.isArray(value)) {
      parts.push(...value.filter((v): v is string => typeof v === "string").map((v) => `· ${v}`));
    }
  }
  return parts.join("\n");
}

function hasDetails(message: AiSessionMessage): boolean {
  if (message.role !== "assistant") return false;
  const content = message.content as Record<string, unknown> | null;
  return !!content && content.type !== "answer" && content.type !== "generatedText" && !!content.payload;
}

function detailsOf(message: AiSessionMessage): string {
  const content = message.content as Record<string, unknown> | null;
  return JSON.stringify(content?.payload ?? content, null, 2);
}

function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleTimeString();
}
</script>

<style scoped>
.conversation {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  padding: var(--gw-space-3);
}

.conversation-empty {
  margin: auto;
}

.load-earlier {
  align-self: center;
}

.message-row {
  display: flex;
}

.message-row.user {
  justify-content: flex-end;
}

.message-bubble {
  max-width: 88%;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  padding: var(--gw-space-2) var(--gw-space-3);
  border-radius: var(--gw-radius-md);
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
}

.message-row.user .message-bubble {
  background: var(--gw-bg-hover);
}

.message-meta {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.message-text {
  font-size: var(--gw-text-sm);
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}

.message-details :deep(.n-collapse-item__header) {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.details-pre {
  margin: 0;
  font-size: var(--gw-text-xs);
  font-family: var(--gw-font-mono);
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 240px;
  overflow-y: auto;
}

.tool-card {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  padding: var(--gw-space-2) var(--gw-space-3);
  border-radius: var(--gw-radius-md);
  border: 1px dashed var(--gw-border);
  background: var(--gw-bg-panel);
}

.tool-card-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.tool-name {
  font-size: var(--gw-text-sm);
}

.tool-meta,
.tool-args,
.tool-result {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.tool-result {
  white-space: pre-wrap;
  word-break: break-word;
}

.tool-error {
  font-size: var(--gw-text-xs);
  color: var(--gw-error);
}

.mono {
  font-family: var(--gw-font-mono);
}
</style>
