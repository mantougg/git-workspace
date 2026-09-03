<template>
  <div class="runtime-scope">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-select
          v-model:value="selectedApp"
          :options="appOptions"
          placeholder="选择 Runtime 应用"
          style="width: 220px"
          @update:value="onAppChange"
        />
        <n-button
          :disabled="!selectedApp"
          :loading="previewing"
          @click="onPreview"
        >
          <template #icon><n-icon><EyeOutline /></n-icon></template>
          预览闭包
        </n-button>
        <n-button
          type="primary"
          :disabled="!selectedApp"
          :loading="saving"
          @click="onSave"
        >
          <template #icon><n-icon><CheckmarkOutline /></n-icon></template>
          保存 Scope
        </n-button>
      </div>
    </div>

    <div v-if="!selectedApp" class="empty-tip">
      选择左侧 Runtime 应用后，在此调整其构建范围（Runtime Closure）。
    </div>

    <template v-else>
      <!-- Mode selection (§15) -->
      <Panel title="Scope 模式">
        <n-radio-group v-model:value="mode" @update:value="onModeChange">
          <n-radio-button value="auto">Auto</n-radio-button>
          <n-radio-button value="manual">Manual</n-radio-button>
          <n-radio-button value="hybrid">Hybrid</n-radio-button>
        </n-radio-group>
        <div class="mode-desc">
          {{
            mode === "auto"
              ? "自动：仅沿源码依赖求最小闭包（推荐，构建范围最小）。"
              : mode === "manual"
                ? "手动：完全由下方勾选决定构建模块集合。"
                : "混合：以 Auto 闭包为基础，勾选增删模块（取消勾选 = 从闭包剔除，勾选 = 额外纳入）。"
          }}
        </div>
        <div class="closure-summary" v-if="preview">
          <n-tag size="small" :bordered="false">
            闭包 {{ preview.closure.projects.length }} 个模块
          </n-tag>
          <n-tag size="small" :type="preview.cacheHit ? 'success' : 'warning'" :bordered="false">
            {{ preview.cacheHit ? "fingerprint 缓存命中" : "本次计算" }}
          </n-tag>
          <span class="mono fingerprint">graph fingerprint: {{ preview.closure.graphFingerprint }}</span>
        </div>
      </Panel>

      <!-- Module checklist -->
      <Panel class="module-section">
        <template #header>
          <span>模块（{{ store.projects.length }} 个 workspace 源码项目 · 已勾选 {{ checkedCount }}）</span>
        </template>
        <template #actions>
          <div class="check-actions" v-if="mode !== 'auto'">
            <n-button size="small" @click="checkAll">全选</n-button>
            <n-button size="small" @click="checkNone">全不选</n-button>
          </div>
        </template>
        <n-scrollbar class="module-scroll">
          <div class="module-list">
            <div
              v-for="p in store.projects"
              :key="p.projectId"
              class="module-item"
              :class="{ 'in-closure': !modeDisabled && isChecked(p.projectId) }"
            >
              <n-checkbox
                :checked="isChecked(p.projectId)"
                :disabled="modeDisabled"
                @update:checked="(val: boolean) => onToggle(p.projectId, val)"
              >
                <span class="module-name">{{ p.coordinates.artifactId }}</span>
                <span class="module-path mono">{{ p.path }}</span>
              </n-checkbox>
              <span class="module-coords mono">
                {{ p.coordinates.groupId }}:{{ p.coordinates.version }}
              </span>
            </div>
            <div v-if="store.projects.length === 0" class="module-empty">
              项目索引为空，请先在 Dashboard 执行「解析依赖」。
            </div>
          </div>
        </n-scrollbar>
      </Panel>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useMessage } from "naive-ui";
import { CheckmarkOutline, EyeOutline } from "@vicons/ionicons5";
import Panel from "@/components/shell/Panel.vue";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import * as runtimeApi from "@/api/runtime";
import type { ClosurePreview, RuntimeApplicationConfig } from "@/types/runtime";
import type { RuntimeScope } from "@/types/maven";
import { errMsg } from "@/utils/error";

type ScopeMode = "auto" | "manual" | "hybrid";

const message = useMessage();
const { store } = useRuntimeWorkspace();

const selectedApp = ref<string | null>(null);
const configDetail = ref<RuntimeApplicationConfig | null>(null);

// F-XX：自动选中第一个应用（用户反馈：作用域页面留空体验差）。
watch(
  () => store.configs,
  (configs) => {
    if (!selectedApp.value && configs.length > 0) {
      selectedApp.value = configs[0].name;
    }
  },
  { immediate: true },
);
const mode = ref<ScopeMode>("auto");
const checkedIds = ref<Set<number>>(new Set());
/** Auto 闭包基准（Hybrid 的剔除集合与「全选」参考）。 */
const autoClosureIds = ref<Set<number>>(new Set());
const preview = ref<ClosurePreview | null>(null);
const previewing = ref(false);
const saving = ref(false);

const modeDisabled = computed(() => mode.value === "auto");
const checkedCount = computed(() => checkedIds.value.size);

const appOptions = computed(() =>
  store.configs.map((c) => ({ label: c.name, value: c.name })),
);

function isChecked(id: number): boolean {
  return checkedIds.value.has(id);
}

function onToggle(id: number, checked: boolean) {
  const next = new Set(checkedIds.value);
  if (checked) next.add(id);
  else next.delete(id);
  checkedIds.value = next;
}

function checkAll() {
  checkedIds.value = new Set(store.projects.map((p) => p.projectId));
}

function checkNone() {
  checkedIds.value = new Set();
}

function scopeFromState(): RuntimeScope {
  switch (mode.value) {
    case "auto":
      return { mode: "auto" };
    case "manual":
      return { mode: "manual", projectIds: [...checkedIds.value] };
    case "hybrid":
      return {
        mode: "hybrid",
        includeProjectIds: [...checkedIds.value],
        excludeProjectIds: [...autoClosureIds.value].filter(
          (id) => !checkedIds.value.has(id),
        ),
      };
  }
}

/** 用配置中的 scope 初始化 UI 状态。 */
function initFromConfig(config: RuntimeApplicationConfig) {
  const scope = config.scope ?? { mode: "auto" as const };
  mode.value = scope.mode;
  switch (scope.mode) {
    case "auto":
      checkedIds.value = new Set(autoClosureIds.value);
      break;
    case "manual":
      checkedIds.value = new Set(scope.projectIds);
      break;
    case "hybrid":
      checkedIds.value = new Set([
        ...scope.includeProjectIds,
        ...autoClosureIds.value,
      ]);
      break;
  }
}

async function loadAutoBase() {
  if (store.workspaceId == null || !configDetail.value) return;
  try {
    const result = await runtimeApi.runtimeGetClosure(
      store.workspaceId,
      configDetail.value.project,
      { mode: "auto" },
    );
    autoClosureIds.value = new Set(
      result.closure.projects.map((p) => p.projectId),
    );
    preview.value = result;
    return result;
  } catch (e) {
    console.error("R-13: load auto closure base failed:", e);
    return null;
  }
}

async function onAppChange() {
  if (!selectedApp.value || store.workspaceId == null) return;
  preview.value = null;
  try {
    configDetail.value = await store.loadConfigDetail(selectedApp.value);
    // Hybrid 的剔除集合需要 Auto 基准；先静默加载。
    await loadAutoBase();
    initFromConfig(configDetail.value);
  } catch (e) {
    message.error("加载配置失败：" + errMsg(e));
  }
}

async function onModeChange() {
  if (mode.value === "auto") {
    // Auto：以后端闭包结果为准（只读）。
    if (!autoClosureIds.value.size) {
      const result = await loadAutoBase();
      if (result) checkedIds.value = new Set(result.closure.projects.map((p) => p.projectId));
      return;
    }
    checkedIds.value = new Set(autoClosureIds.value);
  } else if (mode.value === "hybrid") {
    // Hybrid：Auto 基准 ∪ 当前勾选。
    if (!autoClosureIds.value.size) {
      await loadAutoBase();
    }
    checkedIds.value = new Set([...checkedIds.value, ...autoClosureIds.value]);
  }
  // manual：保持当前勾选，直接可编辑。
}

async function onPreview() {
  if (store.workspaceId == null || !configDetail.value) return;
  previewing.value = true;
  try {
    preview.value = await runtimeApi.runtimeGetClosure(
      store.workspaceId,
      configDetail.value.project,
      scopeFromState(),
    );
  } catch (e) {
    message.error("预览失败：" + errMsg(e));
  } finally {
    previewing.value = false;
  }
}

async function onSave() {
  if (!configDetail.value) return;
  saving.value = true;
  try {
    const next: RuntimeApplicationConfig = {
      ...configDetail.value,
      scope: scopeFromState(),
    };
    await store.saveConfig(next);
    configDetail.value = next;
    message.success("Scope 已保存，下次构建/启动生效");
  } catch (e) {
    message.error("保存失败：" + errMsg(e));
  } finally {
    saving.value = false;
  }
}

onMounted(async () => {
  // 从 Dashboard 带参进入时直接选中该应用。
  const name = new URLSearchParams(window.location.search).get("name");
  if (name && store.configs.some((c) => c.name === name)) {
    selectedApp.value = name;
    await onAppChange();
  }
});
</script>

<style scoped>
.runtime-scope {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
  overflow: hidden;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2);
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
  flex-wrap: wrap;
}
/* D-10：section 外壳已替换为 Panel 组件 */
.module-section {
  margin-top: var(--gw-space-3);
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.check-actions {
  display: flex;
  gap: 6px;
}
.mode-desc {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin-top: 8px;
}
.closure-summary {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-top: 10px;
  flex-wrap: wrap;
}
.fingerprint {
  font-size: 11px;
  color: var(--gw-text-dim);
}
.module-scroll {
  flex: 1;
  min-height: 0;
}
.module-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(380px, 1fr));
  gap: 4px 16px;
}
.module-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-2);
  padding: 6px 8px;
  border-radius: 6px;
}
.module-item.in-closure {
  background: var(--gw-bg-hover);
}
.module-name {
  font-weight: 600;
  font-size: 13px;
  margin-right: 8px;
}
.module-path {
  font-size: 11px;
  color: var(--gw-text-dim);
}
.module-coords {
  font-size: 11px;
  color: var(--gw-text-dim);
  flex-shrink: 0;
}
.module-empty {
  text-align: center;
  color: var(--gw-text-dim);
  font-size: 12px;
  padding: 24px 0;
}
.empty-tip {
  text-align: center;
  color: var(--gw-text-dim);
  font-size: 13px;
  padding: 48px 0;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
</style>
