<template>
  <div class="runtime-deps">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button text @click="goBack">
          <template #icon><n-icon><ArrowBackOutline /></n-icon></template>
          返回
        </n-button>
        <n-select
          v-model:value="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          :options="workspaceOptions"
          @update:value="selectWorkspace"
        />
        <n-button :loading="loading" @click="reload">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-button @click="router.push({ name: 'runtime-dashboard' })">
          <template #icon><n-icon><SpeedometerOutline /></n-icon></template>
          Dashboard
        </n-button>
        <n-button @click="router.push({ name: 'runtime-scope' })">
          <template #icon><n-icon><SettingsOutline /></n-icon></template>
          Scope
        </n-button>
      </div>
    </div>

    <!-- Summary chips -->
    <div class="chips">
      <span class="chip chip-source">
        源码依赖 <b>{{ sourceCount }}</b>
      </span>
      <span class="chip chip-local">
        本地仓库 <b>{{ localCount }}</b>
      </span>
      <span class="chip chip-remote">
        远程仓库 <b>{{ remoteCount }}</b>
      </span>
      <span class="chip chip-total">
        共 <b>{{ totalDependencies }}</b> 条依赖边
        <span v-if="truncated" class="truncated-hint">（已截断，使用项目下钻查看全部）</span>
      </span>
      <span class="chip chip-total">
        模块 <b>{{ projects.length }}</b> · 源码映射 <b>{{ sourceMappingCount }}</b>
      </span>
    </div>

    <div class="main-layout">
      <!-- Left: project list -->
      <div class="project-panel">
        <div class="panel-title">Maven 项目（{{ projects.length }}）</div>
        <n-scrollbar class="project-scroll">
          <div
            v-for="p in projects"
            :key="p.projectId"
            class="project-item"
            :class="{ active: selectedProjectId === p.projectId }"
            @click="onSelectProject(p)"
          >
            <div class="project-name">{{ p.coordinates.artifactId }}</div>
            <div class="project-path mono">{{ p.path }}</div>
            <div class="project-meta">
              <span class="mono">{{ p.coordinates.groupId }}:{{ p.coordinates.version }}</span>
              <n-tag size="small" :bordered="false">{{ p.packaging }}</n-tag>
            </div>
          </div>
          <div v-if="projects.length === 0" class="panel-empty">
            项目索引为空<br />
            请先在 Dashboard 执行「解析依赖」
          </div>
        </n-scrollbar>
      </div>

      <!-- Right: edges / inspection -->
      <div class="detail-panel">
        <!-- Project inspection -->
        <div v-if="inspection" class="inspect-box">
          <div class="panel-title">
            {{ inspection.project.coordinates.artifactId }}
            <span class="muted mono">{{ inspection.project.path }}</span>
          </div>
          <n-descriptions :column="3" bordered size="small" class="inspect-desc">
            <n-descriptions-item label="父项目">
              {{ inspection.parentProjectId ?? "—" }}
            </n-descriptions-item>
            <n-descriptions-item label="子模块">
              {{ inspection.modules.length }}
            </n-descriptions-item>
            <n-descriptions-item label="源码映射">
              {{ inspection.sourceMappings.length }}
            </n-descriptions-item>
          </n-descriptions>
          <div v-if="inspection.sourceMappings.length > 0" class="mapping-list">
            <div v-for="m in inspection.sourceMappings" :key="m.projectId" class="mapping-row">
              <span class="mono">{{ m.coordinates.groupId }}:{{ m.coordinates.artifactId }}</span>
              <span class="mono path">{{ m.projectPath }}</span>
            </div>
          </div>
        </div>

        <!-- Edge filter -->
        <div class="filter-row">
          <span class="filter-label">来源过滤</span>
          <n-radio-group v-model:value="sourceFilter" size="small">
            <n-radio-button value="">全部</n-radio-button>
            <n-radio-button value="workspaceSource">源码</n-radio-button>
            <n-radio-button value="localRepository">本地仓库</n-radio-button>
            <n-radio-button value="remoteRepository">远程仓库</n-radio-button>
          </n-radio-group>
          <span v-if="selectedProjectId" class="filter-project">
            仅看：<b>{{ selectedProjectName }}</b>
            <n-button size="small" text type="primary" @click="clearProject">清除</n-button>
          </span>
        </div>

        <!-- Edges table -->
        <n-spin :show="loading">
          <n-data-table
            :columns="edgeColumns"
            :data="visibleEdges"
            size="small"
            :max-height="560"
            :row-key="(row: any) => `${row.fromProjectId}-${row.dependency.groupId}-${row.dependency.artifactId}`"
          />
        </n-spin>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useMessage, NTag } from "naive-ui";
import { ArrowBackOutline, RefreshOutline, SpeedometerOutline, SettingsOutline } from "@vicons/ionicons5";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import * as runtimeApi from "@/api/runtime";
import type { MavenProjectNode } from "@/types/maven";
import type { DependencyGraphView, ProjectInspection } from "@/types/runtime";

const router = useRouter();
const message = useMessage();
const { workspaceStore, store, selectedWorkspaceId, selectWorkspace } =
  useRuntimeWorkspace();

const loading = ref(false);
const graph = ref<DependencyGraphView | null>(null);
const inspection = ref<ProjectInspection | null>(null);
const selectedProjectId = ref<number | null>(null);
const sourceFilter = ref("");

const projects = computed(() => graph.value?.projects ?? []);
const allEdges = computed(() => graph.value?.dependencies ?? []);
const totalDependencies = computed(() => graph.value?.totalDependencies ?? 0);
const truncated = computed(() => graph.value?.truncated ?? false);
const sourceMappingCount = computed(() => graph.value?.sourceMappings.length ?? 0);

const sourceCount = computed(() => countBySource("workspaceSource"));
const localCount = computed(() => countBySource("localRepository"));
const remoteCount = computed(() => countBySource("remoteRepository"));

const workspaceOptions = computed(() =>
  workspaceStore.workspaces.map((ws: any) => ({ label: ws.name, value: ws.id })),
);

function countBySource(source: string): number {
  return allEdges.value.filter((e) => e.source === source).length;
}

const selectedProjectName = computed(() => {
  const p = projects.value.find((x) => x.projectId === selectedProjectId.value);
  return p?.coordinates.artifactId ?? "";
});

const visibleEdges = computed(() =>
  allEdges.value.filter((e) => {
    if (sourceFilter.value && e.source !== sourceFilter.value) return false;
    if (selectedProjectId.value != null && e.fromProjectId !== selectedProjectId.value)
      return false;
    return true;
  }),
);

function sourceLabel(source: string): string {
  switch (source) {
    case "workspaceSource":
      return "源码";
    case "localRepository":
      return "本地仓库";
    case "remoteRepository":
      return "远程仓库";
    default:
      return source;
  }
}

function sourceTagType(source: string): "success" | "warning" | "info" {
  switch (source) {
    case "workspaceSource":
      return "success";
    case "localRepository":
      return "warning";
    default:
      return "info";
  }
}

function coords(d: { groupId: string; artifactId: string; version: string | null }): string {
  const v = d.version ? `:${d.version}` : "";
  return `${d.groupId}:${d.artifactId}${v}`;
}

function reasonLabel(reason: string): string {
  const map: Record<string, string> = {
    workspaceExactMatch: "workspace 精确匹配",
    localArtifactExists: "本地仓库存在",
    remoteArtifactMissingLocally: "本地缺失，构建时远程解析",
    versionNotExactForSource: "版本与源码不精确匹配",
    workspaceVersionMismatch: "workspace 版本不一致",
    ambiguousWorkspaceCoordinate: "workspace 坐标歧义",
    missingVersion: "缺少版本",
  };
  return map[reason] ?? reason;
}

const edgeColumns = [
  {
    title: "来源",
    width: 110,
    render(row: any) {
      return h(
        NTag,
        { size: "small", bordered: false, type: sourceTagType(row.source) },
        { default: () => sourceLabel(row.source) },
      );
    },
  },
  {
    title: "依赖",
    minWidth: 220,
    render(row: any) {
      const children: any[] = [h("span", { class: "mono" }, coords(row.dependency))];
      if (row.dependency.optional) {
        children.push(
          h(
            NTag,
            { size: "small", bordered: false, type: "info", class: "opt-tag" },
            { default: () => "optional" },
          ),
        );
      }
      return children;
    },
  },
  {
    title: "Scope",
    width: 90,
    render(row: any) {
      return h("span", { class: "mono" }, row.dependency.scope);
    },
  },
  {
    title: "解析路径 / 说明",
    minWidth: 260,
    ellipsis: { tooltip: true },
    render(row: any) {
      if (row.resolvedPath) {
        return h("span", { class: "mono" }, row.resolvedPath);
      }
      return h("span", { class: "muted" }, reasonLabel(row.reason));
    },
  },
];

async function reload() {
  if (store.workspaceId == null) return;
  loading.value = true;
  try {
    graph.value = await runtimeApi.runtimeGetDependencyGraph(store.workspaceId);
  } catch (e) {
    message.error("加载依赖图失败：请先执行「解析依赖」");
    console.error("R-13: load dependency graph failed:", e);
  } finally {
    loading.value = false;
  }
}

async function onSelectProject(p: MavenProjectNode) {
  selectedProjectId.value = p.projectId;
  inspection.value = null;
  if (store.workspaceId == null) return;
  try {
    inspection.value = await runtimeApi.runtimeInspectProject(
      store.workspaceId,
      p.path,
    );
  } catch (e) {
    console.error("R-13: inspect project failed:", e);
  }
}

function clearProject() {
  selectedProjectId.value = null;
  inspection.value = null;
}

function goBack() {
  router.push({ name: "runtime-dashboard" });
}

onMounted(reload);
</script>

<style scoped>
.runtime-deps {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  gap: 12px;
  overflow: hidden;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
}
.chips {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
.chip {
  font-size: 12px;
  padding: 4px 10px;
  border-radius: 12px;
  border: 1px solid var(--el-border-color);
  color: var(--el-text-color-regular);
}
.chip b {
  font-size: 13px;
  margin: 0 2px;
}
.chip-source {
  border-color: var(--el-color-success);
  color: var(--el-color-success);
}
.chip-local {
  border-color: var(--el-color-warning);
  color: var(--el-color-warning);
}
.chip-remote {
  border-color: var(--el-color-info);
  color: var(--el-color-info);
}
.chip-total {
  background: var(--el-fill-color-light);
}
.truncated-hint {
  color: var(--el-color-warning);
}
.main-layout {
  display: flex;
  gap: 12px;
  flex: 1;
  min-height: 0;
}
.project-panel {
  width: 300px;
  flex-shrink: 0;
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.detail-panel {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  overflow: auto;
}
.panel-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 8px;
}
.project-scroll {
  flex: 1;
  min-height: 0;
}
.project-item {
  padding: 8px 10px;
  border-radius: 6px;
  cursor: pointer;
  margin-bottom: 4px;
  border: 1px solid transparent;
}
.project-item:hover {
  background: var(--el-fill-color-light);
}
.project-item.active {
  border-color: var(--el-color-primary);
  background: var(--el-color-primary-light-9);
}
.project-name {
  font-weight: 600;
  font-size: 13px;
}
.project-path {
  font-size: 11px;
  color: var(--el-text-color-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.project-meta {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--el-text-color-secondary);
  margin-top: 2px;
}
.panel-empty {
  text-align: center;
  color: var(--el-text-color-secondary);
  font-size: 12px;
  padding: 24px 0;
  line-height: 1.8;
}
.inspect-box {
  border: 1px dashed var(--el-border-color);
  border-radius: 6px;
  padding: 8px 10px;
}
.inspect-desc {
  margin-bottom: 6px;
}
.mapping-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.mapping-row {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 12px;
  padding: 2px 0;
}
.mapping-row .path {
  color: var(--el-color-primary);
}
.filter-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.filter-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}
.filter-project {
  font-size: 12px;
  color: var(--el-text-color-regular);
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
}
.muted {
  color: var(--el-text-color-secondary);
}
.opt-tag {
  margin-left: 6px;
}
</style>
