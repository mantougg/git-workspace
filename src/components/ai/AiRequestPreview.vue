<template>
  <n-modal
    v-model:show="visible"
    preset="card"
    title="发送前预览"
    style="width: 720px"
    :mask-closable="!confirming"
  >
    <n-spin :show="loading">
      <div v-if="preview" class="body">
        <!-- 概要：Provider/模型、请求类型、目标范围、网络（§10.1） -->
        <div class="summary">
          <div class="summary-row">
            <span class="label">Provider / 模型</span>
            <span>{{ preview.providerName }} · {{ preview.modelName }}（{{ preview.modelId }}）</span>
          </div>
          <div class="summary-row">
            <span class="label">请求类型</span>
            <span>{{ taskKindLabel }}</span>
          </div>
          <div class="summary-row">
            <span class="label">目标范围</span>
            <span class="mono">{{ targetLabel }}</span>
          </div>
          <div class="summary-row">
            <span class="label">网络</span>
            <n-tag size="small" :type="preview.usesNetwork ? 'warning' : 'default'" :bordered="false">
              {{ preview.usesNetwork ? "会使用网络（在线 Provider）" : "不使用网络（本地 Provider）" }}
            </n-tag>
          </div>
        </div>

        <!-- Secret 检测结果（§10.2） -->
        <n-alert v-if="preview.blocked" type="error" :show-icon="false" class="block-alert">
          <div v-for="(reason, i) in preview.blockReasons" :key="i">{{ reason }}</div>
        </n-alert>
        <n-alert
          v-else-if="preview.secret.maskedSources.length > 0"
          type="info"
          :show-icon="false"
          class="block-alert"
        >
          已自动脱敏 {{ preview.secret.maskedSources.length }} 个条目（命中内容已替换为 ***）。
        </n-alert>
        <div v-if="preview.secret.findings.length > 0" class="findings">
          <div
            v-for="f in preview.secret.findings"
            :key="f.sourceId"
            class="finding-row"
          >
            <n-tag size="small" type="error" :bordered="false">Secret</n-tag>
            <span class="finding-name">{{ f.displayName }}</span>
            <span class="finding-kinds">{{ f.kinds.join("、") }}（{{ f.count }} 处）</span>
          </div>
        </div>
        <n-checkbox
          v-if="preview.secret.warnPending"
          :checked="warnAcked"
          class="warn-ack"
          @update:checked="onWarnAck"
        >
          我已知晓以上敏感信息提示，仍要发送（内容不会脱敏）
        </n-checkbox>

        <!-- 内容清单（每项字符数与估算 token；截断/排除可见，§8.2） -->
        <div class="items-header">
          <span>内容清单（{{ includedCount }} / {{ preview.items.length }} 项发送）</span>
          <span class="totals">
            合计 {{ preview.totalChars }} 字符 ≈ {{ preview.totalEstimatedTokens }} token /
            预算 {{ preview.budgetTokens }}
          </span>
        </div>
        <div class="items">
          <div
            v-for="item in preview.items"
            :key="item.sourceId"
            class="item-row"
            :class="{ excluded: item.excluded }"
          >
            <n-checkbox
              :checked="!item.excluded"
              :disabled="confirming"
              @update:checked="(v: boolean) => toggleExclusion(item.sourceId, v)"
            />
            <n-tag size="small" :bordered="false" class="kind-tag">{{ item.kind }}</n-tag>
            <span class="item-name" :title="item.sourceId">{{ item.displayName }}</span>
            <span class="item-meta mono">{{ item.charCount }} 字 ≈ {{ item.estimatedTokens }} tok</span>
            <n-tag v-if="item.redacted" size="small" type="info" :bordered="false">已脱敏</n-tag>
            <n-tag v-if="item.truncated" size="small" type="warning" :bordered="false">已截断</n-tag>
            <n-tag v-if="item.excluded" size="small" type="error" :bordered="false">
              已排除{{ exclusionReasonLabel(item.exclusionReason) }}
            </n-tag>
          </div>
        </div>

        <!-- 预计请求次数 / 成本 / 内容 hash -->
        <div class="footer-info">
          <span>预计请求次数：{{ preview.estimatedRequests }}</span>
          <span>成本估算：{{ preview.costEstimate ?? "—（无定价数据）" }}</span>
          <span class="mono">内容 hash：{{ shortHash }}</span>
        </div>
      </div>
      <n-empty v-else-if="!loading" description="暂无预览数据" />
    </n-spin>

    <template #footer>
      <div class="footer-actions">
        <n-button :disabled="confirming" @click="visible = false">取消</n-button>
        <n-button
          type="primary"
          :disabled="!preview || preview.blocked || loading"
          :loading="confirming"
          @click="emit('confirm')"
        >
          确认发送
        </n-button>
      </div>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { AiContextPreview, ExclusionReason } from "@/types/ai";

const props = withDefaults(
  defineProps<{
    preview: AiContextPreview | null;
    loading?: boolean;
    confirming?: boolean;
  }>(),
  { loading: false, confirming: false },
);

const visible = defineModel<boolean>({ default: false });

const emit = defineEmits<{
  /** 用户点击「确认发送」（父级提交 preview.request 给 ai_submit_request）。 */
  confirm: [];
  /** 用户切换条目排除状态（父级用新 exclusions 重建 Preview，§7.3）。 */
  toggleExclusion: [sourceId: string, included: boolean];
  /** 用户勾选 Warn 确认（父级用 warnConfirmed 重建 Preview，§10.2）。 */
  confirmWarn: [];
}>();

const warnAcked = ref(false);
watch(
  () => props.preview?.contentHash,
  () => {
    // Preview 重建后确认状态随之失效（内容已变，需重新确认）。
    warnAcked.value = false;
  },
);

const TASK_KIND_LABELS: Record<string, string> = {
  chat: "应用助手对话",
  runtimeDiagnostic: "Runtime 失败诊断",
  gitReview: "Git Code Review",
  commitMessage: "提交信息生成",
  conflict: "冲突解决建议",
};

const taskKindLabel = computed(
  () => TASK_KIND_LABELS[props.preview?.taskKind ?? ""] ?? props.preview?.taskKind ?? "",
);

const targetLabel = computed(() => {
  const t = props.preview?.target;
  if (!t) return "—";
  const parts: string[] = [];
  if (t.workspaceName) parts.push(`Workspace「${t.workspaceName}」`);
  if (t.repositoryPaths.length > 0) {
    parts.push(`仓库 ${t.repositoryPaths.join("、")}`);
  } else if (t.repoPath) {
    parts.push(`仓库 ${t.repoPath}`);
  }
  if (t.runtimeName) parts.push(`Runtime「${t.runtimeName}」进程 #${t.processId ?? "?"}`);
  return parts.length > 0 ? parts.join(" · ") : "（无特定目标）";
});

const includedCount = computed(
  () => props.preview?.items.filter((i) => !i.excluded).length ?? 0,
);

const shortHash = computed(() => props.preview?.contentHash.slice(0, 12) ?? "");

function exclusionReasonLabel(reason: ExclusionReason | null | undefined): string {
  switch (reason) {
    case "user":
      return "（手动）";
    case "budgetOverflow":
      return "（预算超限）";
    case "secretPolicy":
      return "（Secret 策略）";
    default:
      return "";
  }
}

function toggleExclusion(sourceId: string, included: boolean) {
  emit("toggleExclusion", sourceId, included);
}

function onWarnAck(checked: boolean) {
  warnAcked.value = checked;
  if (checked) emit("confirmWarn");
}
</script>

<style scoped>
.body {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.summary {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.summary-row {
  display: flex;
  gap: var(--gw-space-3);
  font-size: var(--gw-text-sm);
  align-items: center;
}

.label {
  width: 110px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-xs);
}

.block-alert {
  margin-bottom: var(--gw-space-1);
}

.findings {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.finding-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  font-size: var(--gw-text-sm);
}

.finding-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.finding-kinds {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-xs);
}

.warn-ack {
  font-size: var(--gw-text-sm);
}

.items-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
}

.totals {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-xs);
}

.items {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  max-height: 320px;
  overflow-y: auto;
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  padding: var(--gw-space-2);
  background: var(--gw-bg-panel);
}

.item-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  font-size: var(--gw-text-sm);
}

.item-row.excluded .item-name {
  color: var(--gw-text-dim);
  text-decoration: line-through;
}

.kind-tag {
  flex-shrink: 0;
  font-family: var(--gw-font-mono);
}

.item-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.item-meta {
  flex-shrink: 0;
  color: var(--gw-text-dim);
}

.footer-info {
  display: flex;
  gap: var(--gw-space-4);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.footer-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--gw-space-2);
}
</style>
