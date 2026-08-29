<template>
  <div class="section">
    <div class="stats">
      <n-statistic label="Provider" :value="`${summary?.enabledProviderCount ?? 0}/${summary?.providerCount ?? 0}`">
        <template #suffix><span class="stat-suffix">启用/总数</span></template>
      </n-statistic>
      <n-statistic label="模型" :value="`${summary?.enabledModelCount ?? 0}/${summary?.modelCount ?? 0}`">
        <template #suffix><span class="stat-suffix">启用/总数</span></template>
      </n-statistic>
      <n-statistic label="会话内凭证" :value="summary?.sessionCredentialCount ?? 0">
        <template #suffix><span class="stat-suffix">不落盘</span></template>
      </n-statistic>
      <n-statistic label="历史 AI 审查" :value="summary?.legacyReviewCount ?? 0" />
      <n-statistic label="历史 AI 任务" :value="summary?.legacyTaskCount ?? 0" />
    </div>

    <n-alert
      :type="summary?.osCredentialStoreAvailable ? 'success' : 'warning'"
      :show-icon="false"
    >
      <template v-if="summary?.osCredentialStoreAvailable">
        OS 凭证存储可用：API Key 持久保存于系统凭证管理器。
      </template>
      <template v-else>
        OS 凭证存储不可用：只能选择「仅本次会话」临时保存 API Key（进程退出即清除，不落盘）。
      </template>
    </n-alert>

    <div class="actions">
      <n-button @click="openLogsDir">
        <template #icon><n-icon><FolderOpenOutline /></n-icon></template>
        打开日志目录（ai.log）
      </n-button>
      <n-button :loading="loading" @click="emit('refresh')">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
    </div>

    <n-alert type="info" :show-icon="false">
      请求审计（请求次数、token 用量、最近错误）与缓存清理随 AI-04「Session /
      Request Audit / 结果缓存」落地。当前展示的是原型遗留表（ai_reviews /
      ai_tasks）的历史行数，兼容保留、不做破坏性删除。
    </n-alert>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useMessage } from "naive-ui";
import { FolderOpenOutline, RefreshOutline } from "@vicons/ionicons5";
import { openLogs } from "@/api/logs";
import { errMsg } from "@/utils/error";
import type { AiSettingsSummary } from "@/types/ai";

defineProps<{ summary: AiSettingsSummary | null }>();
const emit = defineEmits<{ refresh: [] }>();

const message = useMessage();
const loading = ref(false);

async function openLogsDir() {
  try {
    await openLogs();
  } catch (e) {
    message.error("打开日志目录失败: " + errMsg(e));
  }
}
</script>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: var(--gw-space-3);
}

.stat-suffix {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin-left: 4px;
}

.actions {
  display: flex;
  gap: var(--gw-space-2);
}
</style>
