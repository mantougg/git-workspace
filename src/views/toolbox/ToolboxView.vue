<template>
  <div class="toolbox-view">
    <Toolbar>
      <!-- 详情态的返回（视图内模式切换，非路由导航） -->
      <n-button v-if="activeTool" text size="small" @click="activeTool = null">
        <template #icon><n-icon><ChevronBackOutline /></n-icon></template>
        工具箱
      </n-button>
      <n-input
        v-model:value="keyword"
        size="small"
        clearable
        placeholder="搜索工具…"
        class="tool-search"
      >
        <template #prefix>
          <n-icon><SearchOutline /></n-icon>
        </template>
      </n-input>
      <template #right>
        <span class="hint">新工具在 registry.ts 注册即出现</span>
      </template>
    </Toolbar>

    <!-- 首页：工具卡片网格 -->
    <div v-if="!activeTool" class="card-grid">
      <button
        v-for="tool in filtered"
        :key="tool.id"
        class="tool-card"
        @click="activeTool = tool"
      >
        <n-icon :size="22" class="tool-card-icon">
          <component :is="tool.icon" />
        </n-icon>
        <span class="tool-card-title">{{ tool.title }}</span>
        <span class="tool-card-desc">{{ tool.description }}</span>
      </button>
      <n-empty
        v-if="filtered.length === 0"
        size="small"
        description="没有匹配的工具"
        class="grid-empty"
      />
    </div>

    <!-- 详情：左侧工具列表 + 右侧工具面板（纵向占满） -->
    <div v-else class="toolbox-body">
      <Panel class="fill-panel tool-list-panel">
        <div class="tool-list">
          <button
            v-for="tool in filtered"
            :key="tool.id"
            class="tool-item"
            :class="{ active: tool.id === activeTool.id }"
            @click="activeTool = tool"
          >
            <n-icon :size="16" class="tool-icon">
              <component :is="tool.icon" />
            </n-icon>
            <span class="tool-title">{{ tool.title }}</span>
          </button>
          <n-empty
            v-if="filtered.length === 0"
            size="small"
            description="没有匹配的工具"
          />
        </div>
      </Panel>

      <Panel class="fill-panel tool-content-panel" :title="activeTool.title">
        <component :is="activeTool.component" />
      </Panel>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { NButton, NEmpty, NIcon, NInput } from "naive-ui";
import { ChevronBackOutline, SearchOutline } from "@vicons/ionicons5";
import Panel from "@/components/shell/Panel.vue";
import Toolbar from "@/components/shell/Toolbar.vue";
import { TOOLS, type ToolboxTool } from "./registry";

const keyword = ref("");
/** null = 卡片首页；非空 = 详情态（左列表 + 右面板）。 */
const activeTool = ref<ToolboxTool | null>(null);

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return TOOLS;
  return TOOLS.filter(
    (t) =>
      t.title.toLowerCase().includes(kw) ||
      t.keywords.some((k) => k.toLowerCase().includes(kw)),
  );
});
</script>

<style scoped>
.toolbox-view {
  height: 100%;
  box-sizing: border-box;
  padding: var(--gw-space-4) calc(var(--gw-space-4) + var(--gw-space-2));
  display: flex;
  flex-direction: column;
}

.tool-search {
  width: 240px;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

/* ── 首页卡片网格 ── */
.card-grid {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: var(--gw-space-3);
  align-content: start;
}

.tool-card {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: var(--gw-space-2);
  padding: var(--gw-space-4);
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  text-align: left;
  cursor: pointer;
  transition: border-color 0.15s;
}

.tool-card:hover {
  border-color: var(--gw-accent);
}

.tool-card-icon {
  color: var(--gw-accent);
}

.tool-card-title {
  font-size: var(--gw-text-lg);
  font-weight: 600;
  color: var(--gw-text);
}

.tool-card-desc {
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
  line-height: 1.5;
}

.grid-empty {
  grid-column: 1 / -1;
}

/* ── 详情态：左右分栏纵向占满 ── */
.toolbox-body {
  flex: 1;
  min-height: 0;
  display: flex;
  gap: var(--gw-space-3);
}

.fill-panel {
  height: 100%;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
}

.fill-panel :deep(.panel-body) {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.tool-list-panel {
  width: 200px;
  flex-shrink: 0;
}

.tool-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.tool-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: var(--gw-space-2);
  border: none;
  border-radius: var(--gw-radius-md);
  background: transparent;
  color: var(--gw-text);
  font-size: var(--gw-text-md);
  text-align: left;
  cursor: pointer;
}

.tool-item:hover {
  background: var(--gw-bg-hover);
}

.tool-item.active {
  background: color-mix(in srgb, var(--gw-accent) 12%, transparent);
  color: var(--gw-accent);
  font-weight: 600;
}

.tool-icon {
  flex-shrink: 0;
}

.tool-title {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tool-content-panel {
  flex: 1;
  min-width: 0;
}
</style>
