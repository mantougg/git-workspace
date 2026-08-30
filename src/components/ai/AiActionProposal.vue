<template>
  <n-card size="small" class="proposal-card" :bordered="true">
    <template #header>
      <div class="proposal-header">
        <span>{{ actionLabel }}</span>
        <n-tag size="small" :type="riskType" :bordered="false">{{ riskLabel }}
        </n-tag>
      </div>
    </template>

    <div class="proposal-body">
      <div class="status-row">
        <span class="status-label">状态</span>
        <n-tag size="small" :type="statusType" :bordered="false">{{ statusLabel }}
        </n-tag>
        <span class="expires mono">有效期至 {{ formatDate(proposal.expiresAt) }}</span>
      </div>

      <section class="summary-grid">
        <div>
          <div class="section-title">执行前</div>
          <p>{{ proposal.beforeSummary }}</p>
        </div>
        <div>
          <div class="section-title">执行后</div>
          <p>{{ proposal.afterSummary }}</p>
        </div>
      </section>

      <div v-if="proposal.affectedRepositories.length" class="scope-block">
        <div class="section-title">影响仓库</div>
        <div v-for="repo in proposal.affectedRepositories" :key="repo" class="mono scope-item">{{ repo }}</div>
      </div>
      <div v-if="proposal.affectedFiles.length" class="scope-block">
        <div class="section-title">影响文件</div>
        <div v-for="file in proposal.affectedFiles" :key="file" class="mono scope-item">{{ file }}</div>
      </div>

      <n-collapse>
        <n-collapse-item title="查看命令预览与 Diff" name="details">
          <pre v-if="proposal.commandPreview" class="preview mono">{{ proposal.commandPreview }}</pre>
          <pre v-if="proposal.diff" class="preview mono">{{ proposal.diff }}</pre>
          <span v-if="!proposal.commandPreview && !proposal.diff" class="muted">无额外预览</span>
        </n-collapse-item>
      </n-collapse>

      <n-checkbox v-if="isHighRisk && proposal.status === 'pending'" v-model:checked="secondConfirmation">
        我已检查影响范围，并确认执行此高风险操作
      </n-checkbox>
    </div>

    <template #footer>
      <div class="proposal-footer">
        <span class="reversible">{{ proposal.reversible ? "支持 Undo" : "不可逆" }}</span>
        <div class="actions">
          <n-button v-if="proposal.status === 'pending'" size="small" @click="emit('reject')">拒绝</n-button>
          <n-button
            v-if="proposal.status === 'pending'"
            size="small"
            type="primary"
            :disabled="isHighRisk && !secondConfirmation"
            :loading="confirming"
            @click="confirm"
          >确认执行</n-button>
          <n-button v-if="proposal.status !== 'pending'" size="small" quaternary @click="emit('detail')">查看详情</n-button>
        </div>
      </div>
    </template>
  </n-card>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import type { ActionProposal } from "@/types/ai";

const props = defineProps<{ proposal: ActionProposal; confirming?: boolean }>();
const emit = defineEmits<{ confirm: [secondConfirmation: boolean]; reject: []; detail: [] }>();
const secondConfirmation = ref(false);
const confirming = computed(() => props.confirming ?? false);
const isHighRisk = computed(() => props.proposal.riskLevel === "high");
const riskLabel = computed(() => ({ low: "低风险", medium: "中风险", high: "高风险" }[props.proposal.riskLevel]));
const riskType = computed(() => ({ low: "success", medium: "warning", high: "error" }[props.proposal.riskLevel] as "success" | "warning" | "error"));
const statusLabel = computed(() => ({ pending: "待确认", confirmed: "已确认", executed: "已提交", rejected: "已拒绝", expired: "已过期" }[props.proposal.status]));
const statusType = computed(() => props.proposal.status === "executed" ? "success" : props.proposal.status === "pending" ? "warning" : "default");
const actionLabel = computed(() => ({ gitCreateCommit: "创建提交", runtimeStart: "启动 Runtime", conflictApply: "应用冲突解决", runtimeUpdateConfig: "更新 Runtime 配置" }[props.proposal.actionKind]));

function confirm() {
  emit("confirm", secondConfirmation.value);
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}
</script>

<style scoped>
.proposal-card { max-width: 100%; }
.proposal-header, .proposal-footer, .status-row, .actions { display: flex; align-items: center; gap: var(--gw-space-2); }
.proposal-header, .proposal-footer { justify-content: space-between; }
.proposal-body { display: flex; flex-direction: column; gap: var(--gw-space-3); }
.status-label, .section-title, .muted, .reversible { color: var(--gw-text-dim); font-size: var(--gw-text-sm); }
.expires { margin-left: auto; color: var(--gw-text-dim); font-size: var(--gw-text-xs); }
.summary-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--gw-space-3); }
.summary-grid p { margin: var(--gw-space-1) 0 0; line-height: 1.45; }
.scope-block { display: flex; flex-direction: column; gap: var(--gw-space-1); }
.scope-item { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--gw-text-xs); }
.preview { margin: var(--gw-space-2) 0 0; max-height: 14rem; overflow: auto; white-space: pre-wrap; font-size: var(--gw-text-xs); }
.mono { font-family: var(--gw-font-mono); }
@media (max-width: 640px) { .summary-grid { grid-template-columns: 1fr; } .expires { margin-left: 0; } .status-row { align-items: flex-start; flex-wrap: wrap; } }
</style>
