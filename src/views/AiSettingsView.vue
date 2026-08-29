<template>
  <div class="ai-settings">
    <div class="toolbar">
      <div class="toolbar-left">
        <span class="page-title">AI 设置</span>
        <n-tag
          v-if="summary && !summary.osCredentialStoreAvailable"
          size="small"
          type="warning"
          :bordered="false"
        >
          OS 凭证存储不可用
        </n-tag>
      </div>
      <div class="toolbar-right">
        <n-button :loading="loading" @click="loadAll">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
    </div>

    <n-spin :show="loading && providers.length === 0">
      <n-tabs type="line" animated class="settings-tabs">
        <n-tab-pane name="providers" tab="Provider">
          <AiProvidersSection
            :providers="providers"
            @refresh="loadAll"
          />
        </n-tab-pane>
        <n-tab-pane name="models" tab="模型">
          <AiModelsSection
            :providers="providers"
            :models="models"
            @refresh="loadAll"
          />
        </n-tab-pane>
        <n-tab-pane name="defaults" tab="任务默认值">
          <AiTaskDefaultsSection
            :providers="providers"
            :models="models"
            :task-defaults="summary?.taskDefaults ?? []"
            @refresh="loadAll"
          />
        </n-tab-pane>
        <n-tab-pane name="privacy" tab="隐私与安全">
          <AiPrivacySection />
        </n-tab-pane>
        <n-tab-pane name="usage" tab="用量与诊断">
          <AiUsageSection :summary="summary" @refresh="loadAll" />
        </n-tab-pane>
        <n-tab-pane name="credentials" tab="凭证">
          <AiCredentialsSection
            :providers="providers"
            :os-store-available="summary?.osCredentialStoreAvailable ?? true"
            @refresh="loadAll"
          />
        </n-tab-pane>
      </n-tabs>
    </n-spin>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { RefreshOutline } from "@vicons/ionicons5";
import AiProvidersSection from "@/components/ai/AiProvidersSection.vue";
import AiModelsSection from "@/components/ai/AiModelsSection.vue";
import AiTaskDefaultsSection from "@/components/ai/AiTaskDefaultsSection.vue";
import AiPrivacySection from "@/components/ai/AiPrivacySection.vue";
import AiUsageSection from "@/components/ai/AiUsageSection.vue";
import AiCredentialsSection from "@/components/ai/AiCredentialsSection.vue";
import { aiGetSettingsSummary, aiListModels, aiListProviders } from "@/api/ai";
import { errMsg } from "@/utils/error";
import type { AiModel, AiProvider, AiSettingsSummary } from "@/types/ai";

const message = useMessage();

const loading = ref(false);
const providers = ref<AiProvider[]>([]);
const models = ref<AiModel[]>([]);
const summary = ref<AiSettingsSummary | null>(null);

/**
 * 加载设置页全部数据。只读配置表（§10：打开 AI 设置不得触发全量
 * Repository 扫描），Key 永不返回前端（凭证实况为布尔标记）。
 */
async function loadAll() {
  loading.value = true;
  try {
    const [p, m, s] = await Promise.all([
      aiListProviders(),
      aiListModels(),
      aiGetSettingsSummary(),
    ]);
    providers.value = p;
    models.value = m;
    summary.value = s;
  } catch (e) {
    message.error("加载 AI 设置失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

onMounted(loadAll);
</script>

<style scoped>
.ai-settings {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.page-title {
  font-size: 15px;
  font-weight: 600;
}

.settings-tabs {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.settings-tabs :deep(.n-tab-pane) {
  overflow-y: auto;
  padding-top: var(--gw-space-2);
}
</style>
