<template>
  <AiRequestPreview
    v-model="previewVisible"
    :preview="diagnostic.preview.value"
    :loading="diagnostic.loading.value"
    :confirming="diagnostic.confirming.value"
    @confirm="diagnostic.confirm"
    @toggle-exclusion="diagnostic.toggleExclusion"
    @confirm-warn="diagnostic.confirmWarn"
  />

  <n-alert v-if="diagnostic.error.value" type="error" class="diagnostic-error">
    <div class="error-row">
      <span>{{ errMsg(diagnostic.error.value) }}</span>
      <span class="error-actions">
        <n-button size="small" @click="retry">重试</n-button>
        <n-button v-if="canConfigure" size="small" @click="emit('configure')">
          打开 AI 设置
        </n-button>
      </span>
    </div>
  </n-alert>

  <n-alert v-if="diagnostic.running.value" type="info" :show-icon="false" class="diagnostic-running">
    正在等待 Runtime 诊断结果…
  </n-alert>

  <AiSuggestionCard
    v-if="diagnostic.snapshot.value?.phase === 'succeeded'"
    :snapshot="diagnostic.snapshot.value"
    :sources="diagnostic.preview.value?.items ?? []"
    :runtime-name="diagnostic.request.value?.runtimeName"
    :process-id="diagnostic.request.value?.processId"
    @retry="retry"
  />
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import AiRequestPreview from "@/components/ai/AiRequestPreview.vue";
import AiSuggestionCard from "@/components/ai/AiSuggestionCard.vue";
import { useRuntimeAiDiagnostic } from "@/composables/useRuntimeAiDiagnostic";
import { errMsg } from "@/utils/error";
import type { RuntimeDiagnosticRequest } from "@/types/ai";

const props = defineProps<{ request: RuntimeDiagnosticRequest | null }>();
const emit = defineEmits<{ configure: [] }>();
const previewVisible = ref(false);
const diagnostic = useRuntimeAiDiagnostic();

const CONFIG_ERROR_CODES = new Set(["AiNotConfigured", "AiCredentialUnavailable"]);

const canConfigure = computed(() => {
  const cause = diagnostic.error.value;
  if (!cause || typeof cause !== "object" || !("code" in cause)) return false;
  const code = (cause as { code?: unknown }).code;
  return typeof code === "string" && CONFIG_ERROR_CODES.has(code);
});

watch(
  () => props.request,
  async (request) => {
    if (!request) return;
    try {
      await diagnostic.open(request);
      previewVisible.value = true;
    } catch {
      previewVisible.value = false;
    }
  },
  { deep: true },
);

async function retry() {
  const request = diagnostic.request.value ?? props.request;
  if (!request) return;
  try {
    await diagnostic.open({ ...request, exclusions: [...(request.exclusions ?? [])] });
    previewVisible.value = true;
  } catch {
    previewVisible.value = false;
  }
}

onUnmounted(diagnostic.dispose);
</script>

<style scoped>
.diagnostic-error,
.diagnostic-running {
  margin-top: var(--gw-space-3);
}
.error-row,
.error-actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}
.error-row {
  justify-content: space-between;
}
@media (max-width: 640px) {
  .error-row {
    align-items: flex-start;
    flex-direction: column;
  }
  .error-actions {
    flex-wrap: wrap;
  }
}
</style>
