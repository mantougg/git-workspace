<template>
  <div class="runtime-deps">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button :loading="loading" @click="reload">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
        <!-- R-20 §45：表格 / 树·图 双视图 -->
        <n-radio-group v-model:value="viewMode" size="small">
          <n-radio-button value="table">表格</n-radio-button>
          <n-radio-button value="tree">树·图（R-20）</n-radio-button>
        </n-radio-group>
      </div>
      <div class="toolbar-right" v-if="viewMode === 'tree'">
        <n-select
          v-model:value="selectedApp"
          :options="appOptions"
          placeholder="选择 Runtime 应用"
          size="small"
          style="width: 200px"
          @update:value="onTreeAppChange"
          clearable
        />
        <n-input
          v-model:value="treeQuery"
          placeholder="搜索模块 / 坐标过滤子图"
          size="small"
          clearable
          style="width: 220px"
        />
        <n-button size="small" @click="onExpandAll">展开全部</n-button>
        <n-button size="small" @click="onCollapseAll">折叠</n-button>
      </div>
    </div>

    <!-- ==================== 表格视图（R-13 原有） ==================== -->
    <template v-if="viewMode === 'table'">
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
    </template>

    <!-- ==================== 树·图视图（R-20 §45） ==================== -->
    <template v-else>
      <!-- 图例：颜色 + 文字双通道（可访问性） -->
      <div class="chips">
        <span class="chip chip-source">● 源码（workspace，含相对路径）</span>
        <span class="chip chip-local">● 本地 Maven</span>
        <span class="chip chip-remote">● 远程 Maven</span>
        <span class="chip chip-total" v-if="treeStats">
          模块 <b>{{ treeStats.modules }}</b> · 库依赖 <b>{{ treeStats.libraries }}</b> ·
          可见 {{ visibleRows.length }} 行 · 渲染 <b>{{ treeStats.renderMs.toFixed(1) }}</b> ms
        </span>
        <span class="chip chip-total" v-if="closurePreview">
          闭包 <b>{{ closurePreview.closure.projects.length }}</b> 模块
          <span :class="closurePreview.cacheHit ? 'truncated-hint' : ''">
            {{ closurePreview.cacheHit ? "（缓存命中）" : "（本次计算）" }}
          </span>
        </span>
      </div>

      <div class="main-layout">
        <!-- Left: tree -->
        <div class="tree-panel">
          <div v-if="!selectedApp" class="panel-empty">
            选择 Runtime 应用后按 §45 层次展示：应用 → 模块 → 库依赖。
          </div>
          <div v-else-if="treeRoot" class="tree-scroll-wrap">
            <n-virtual-list
              :items="visibleRows"
              :item-resizable="false"
              item-key="key"
              :item-size="34"
              class="tree-list"
            >
              <template #default="{ item }">
                <div
                  class="tree-row"
                  :style="{ paddingLeft: 8 + item.depth * 18 + 'px' }"
                  :class="{ selected: selectedNodeKey === item.node.key }"
                  @click="onSelectNode(item.node)"
                >
                  <span
                    v-if="item.hasChildren"
                    class="tree-arrow"
                    @click.stop="onToggleExpand(item.node.key)"
                  >{{ expanded.has(item.node.key) ? "▾" : "▸" }}</span>
                  <span v-else class="tree-arrow leaf">·</span>
                  <n-checkbox
                    v-if="item.node.kind === 'module' && scopeMode !== 'auto'"
                    size="small"
                    :checked="checkedIds.has(item.node.projectId!)"
                    class="tree-check"
                    @update:checked="(v: boolean) => onTreeScopeToggle(item.node.projectId!, v)"
                    @click.stop
                  />
                  <span class="tree-dot" :class="dotClass(item.node.source)"></span>
                  <span class="tree-label" :class="{ 'label-app': item.node.kind === 'app' }">
                    {{ item.node.label }}
                  </span>
                  <span class="tree-coords mono">{{ item.node.coordinates }}</span>
                  <span class="tree-source-tag" :class="dotClass(item.node.source)">
                    {{ sourceShortLabel(item.node) }}
                  </span>
                </div>
              </template>
            </n-virtual-list>
          </div>
          <div v-else class="panel-empty">闭包尚未计算。</div>
        </div>

        <!-- Right: node detail + scope 联动 -->
        <div class="detail-panel">
          <div v-if="selectedNode" class="inspect-box">
            <div class="panel-title">
              {{ selectedNode.label }}
              <n-tag size="small" :bordered="false" :type="tagType(selectedNode.source)">
                {{ sourceShortLabel(selectedNode) }}
              </n-tag>
            </div>
            <n-descriptions :column="1" bordered size="small" class="inspect-desc">
              <n-descriptions-item label="GAV">
                <span class="mono">{{ selectedNode.coordinates }}</span>
              </n-descriptions-item>
              <n-descriptions-item label="版本">
                <span class="mono">{{ selectedNode.version ?? "—" }}</span>
              </n-descriptions-item>
              <n-descriptions-item label="来源">
                {{ sourceLabel(selectedNode.source) }}
              </n-descriptions-item>
              <n-descriptions-item label="路径 / 仓库">
                <span class="mono" v-if="selectedNode.path">{{ selectedNode.path }}</span>
                <span class="muted" v-else>{{ selectedNode.edge ? reasonLabel(selectedNode.edge.reason) : "—" }}</span>
              </n-descriptions-item>
              <n-descriptions-item label="直接依赖">
                {{ selectedNode.children.length }}
              </n-descriptions-item>
            </n-descriptions>
          </div>
          <div v-else class="panel-empty">点击树中节点查看 GAV / 版本 / 来源 / 路径详情。</div>

          <!-- Scope 联动（与 RuntimeScopeView 同语义） -->
          <div class="scope-box" v-if="selectedApp && configDetail">
            <div class="panel-title">Scope 联动</div>
            <n-space align="center" :size="8" wrap>
              <n-radio-group v-model:value="scopeMode" size="small" @update:value="onScopeModeChange">
                <n-radio-button value="auto">Auto</n-radio-button>
                <n-radio-button value="manual">Manual</n-radio-button>
                <n-radio-button value="hybrid">Hybrid</n-radio-button>
              </n-radio-group>
              <n-button
                size="small"
                type="primary"
                :loading="scopeSaving"
                :disabled="scopeMode === 'auto'"
                @click="onSaveScope"
              >
                保存 Scope
              </n-button>
            </n-space>
            <div class="mode-desc">
              {{
                scopeMode === "auto"
                  ? "Auto：闭包由源码依赖自动推导，树中复选框只读；切换 Hybrid / Manual 后可直接在图侧勾选调整闭包。"
                  : scopeMode === "manual"
                    ? "Manual：勾选即纳入构建闭包，保存后生效。"
                    : "Hybrid：以 Auto 闭包为基础，取消勾选 = 剔除，勾选 = 额外纳入。"
              }}
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, h, nextTick, onMounted, ref, watch } from "vue";
import { useMessage, NTag } from "naive-ui";
import { RefreshOutline } from "@vicons/ionicons5";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import { errMsg } from "@/utils/error";
import * as runtimeApi from "@/api/runtime";
import type { MavenProjectNode, RuntimeScope } from "@/types/maven";
import type {
  ClosurePreview,
  DependencyGraphView,
  ProjectInspection,
  RuntimeApplicationConfig,
} from "@/types/runtime";
import {
  buildDependencyTree,
  countUniqueNodes,
  defaultExpanded,
  expandAll,
  filterTree,
  flattenVisible,
  type ExpandedSet,
  type TreeNode,
} from "@/utils/dependencyTree";

const message = useMessage();
const { store } = useRuntimeWorkspace();

const loading = ref(false);
const graph = ref<DependencyGraphView | null>(null);
const inspection = ref<ProjectInspection | null>(null);
const selectedProjectId = ref<number | null>(null);
const sourceFilter = ref("");

// ---------------------------------------------------------------------------
// R-20 §45 树·图视图状态
// ---------------------------------------------------------------------------

type ScopeMode = "auto" | "manual" | "hybrid";

const viewMode = ref<"table" | "tree">("table");
const selectedApp = ref<string | null>(null);
const configDetail = ref<RuntimeApplicationConfig | null>(null);
const treeRoot = ref<TreeNode | null>(null);
const expanded = ref<ExpandedSet>(new Set());
const treeQuery = ref("");
const selectedNodeKey = ref<string | null>(null);
const closurePreview = ref<ClosurePreview | null>(null);
const treeBusy = ref(false);
const renderMs = ref(0);
const visibleRows = ref<{ key: string; node: TreeNode; depth: number; hasChildren: boolean }[]>([]);

/** Scope 草稿（与 RuntimeScopeView 同语义：auto 只读 / manual 勾选集 /
 * hybrid = Auto 基准 ∪ 勾选，剔除 = autoBase − checked）。 */
const scopeMode = ref<ScopeMode>("auto");
const checkedIds = ref<Set<number>>(new Set());
const autoClosureIds = ref<Set<number>>(new Set());
const scopeSaving = ref(false);

const appOptions = computed(() =>
  store.configs.map((c) => ({ label: c.name, value: c.name })),
);

const totalNodes = computed(() => {
  if (!treeRoot.value) return null;
  const { modules, libraries } = countUniqueNodes(treeRoot.value);
  return { modules, libraries };
});
const treeStats = computed(() =>
  totalNodes.value ? { ...totalNodes.value, renderMs: renderMs.value } : null,
);

const selectedNode = computed<TreeNode | null>(() => {
  if (!selectedNodeKey.value || !treeRoot.value) return null;
  return findNode(treeRoot.value, selectedNodeKey.value);
});

function findNode(root: TreeNode, key: string): TreeNode | null {
  if (root.key === key) return root;
  for (const c of root.children) {
    const hit = findNode(c, key);
    if (hit) return hit;
  }
  return null;
}

/** 过滤后的树（搜索命中节点保留祖先与后代）。 */
const filteredRoot = computed(() =>
  treeRoot.value ? filterTree(treeRoot.value, treeQuery.value) : null,
);

/** 渲染测量（验收：100+ 模块帧时间有测量）：展平 + DOM 提交耗时。 */
watch(
  [filteredRoot, expanded],
  async () => {
    if (!filteredRoot.value) {
      visibleRows.value = [];
      return;
    }
    const t0 = performance.now();
    visibleRows.value = flattenVisible(filteredRoot.value, expanded.value);
    await nextTick();
    renderMs.value = performance.now() - t0;
  },
  { immediate: true },
);

function dotClass(source: string): string {
  switch (source) {
    case "workspaceSource":
      return "dot-source";
    case "localRepository":
      return "dot-local";
    default:
      return "dot-remote";
  }
}

function tagType(source: string): "success" | "warning" | "info" {
  if (source === "workspaceSource") return "success";
  if (source === "localRepository") return "warning";
  return "info";
}

function sourceShortLabel(node: TreeNode): string {
  if (node.kind === "library") {
    return node.source === "localRepository" ? "本地" : "远程";
  }
  return "源码";
}

function scopeFromState(): RuntimeScope {
  switch (scopeMode.value) {
    case "auto":
      return { mode: "auto" };
    case "manual":
      return { mode: "manual", projectIds: [...checkedIds.value] };
    case "hybrid":
      return {
        mode: "hybrid",
        includeProjectIds: [...checkedIds.value].filter(
          (id) => !autoClosureIds.value.has(id),
        ),
        excludeProjectIds: [...autoClosureIds.value].filter(
          (id) => !checkedIds.value.has(id),
        ),
      };
  }
}

/** 用配置 scope 初始化勾选状态（对齐 ScopeView）。 */
function initCheckedFromScope(config: RuntimeApplicationConfig, autoIds: Set<number>) {
  const scope = config.scope ?? { mode: "auto" as const };
  scopeMode.value = scope.mode;
  switch (scope.mode) {
    case "auto":
      checkedIds.value = new Set(autoIds);
      break;
    case "manual":
      checkedIds.value = new Set(scope.projectIds);
      break;
    case "hybrid":
      checkedIds.value = new Set([...scope.includeProjectIds, ...autoIds]);
      break;
  }
}

/** 以当前 scope 草稿重算闭包并重建树（Scope 联动闭环）。 */
async function recomputeClosureAndTree() {
  if (!configDetail.value || store.workspaceId == null) return;
  treeBusy.value = true;
  try {
    const preview = await runtimeApi.runtimeGetClosure(
      store.workspaceId,
      configDetail.value.project,
      scopeFromState(),
    );
    closurePreview.value = preview;
    const root = buildDependencyTree(graph.value!, preview.closure);
    treeRoot.value = root;
    expanded.value = defaultExpanded(root);
  } catch (e) {
    console.error("R-20: closure recompute failed:", e);
  } finally {
    treeBusy.value = false;
  }
}

async function onTreeAppChange(name: string | null) {
  if (!name || store.workspaceId == null) {
    configDetail.value = null;
    treeRoot.value = null;
    closurePreview.value = null;
    return;
  }
  if (!graph.value) await reload();
  if (!graph.value) return;
  try {
    configDetail.value = await store.loadConfigDetail(name);
  } catch (e) {
    message.error("加载配置失败：" + errMsg(e));
    return;
  }
  try {
    const auto = await runtimeApi.runtimeGetClosure(
      store.workspaceId,
      configDetail.value.project,
      { mode: "auto" },
    );
    autoClosureIds.value = new Set(auto.closure.projects.map((p) => p.projectId));
  } catch (e) {
    console.error("R-20: auto closure base failed:", e);
    autoClosureIds.value = new Set();
  }
  initCheckedFromScope(configDetail.value, autoClosureIds.value);
  await recomputeClosureAndTree();
}

async function onTreeScopeToggle(projectId: number, checked: boolean) {
  const next = new Set(checkedIds.value);
  if (checked) next.add(projectId);
  else next.delete(projectId);
  checkedIds.value = next;
  // 勾选即预览闭包（closure 缓存热路径，§15/R-03），树即时反映调整。
  await recomputeClosureAndTree();
}

async function onScopeModeChange() {
  if (scopeMode.value === "auto") {
    checkedIds.value = new Set(autoClosureIds.value);
  } else if (scopeMode.value === "hybrid") {
    checkedIds.value = new Set([...checkedIds.value, ...autoClosureIds.value]);
  }
  await recomputeClosureAndTree();
}

async function onSaveScope() {
  if (!configDetail.value) return;
  scopeSaving.value = true;
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
    scopeSaving.value = false;
  }
}

function onToggleExpand(key: string) {
  const next = new Set(expanded.value);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  expanded.value = next;
}

function onExpandAll() {
  if (treeRoot.value) expanded.value = expandAll(treeRoot.value);
}

function onCollapseAll() {
  if (treeRoot.value) expanded.value = defaultExpanded(treeRoot.value);
}

function onSelectNode(node: TreeNode) {
  selectedNodeKey.value = node.key;
}

const projects = computed(() => graph.value?.projects ?? []);
const allEdges = computed(() => graph.value?.dependencies ?? []);
const totalDependencies = computed(() => graph.value?.totalDependencies ?? 0);
const truncated = computed(() => graph.value?.truncated ?? false);
const sourceMappingCount = computed(() => graph.value?.sourceMappings.length ?? 0);

const sourceCount = computed(() => countBySource("workspaceSource"));
const localCount = computed(() => countBySource("localRepository"));
const remoteCount = computed(() => countBySource("remoteRepository"));

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
    key: "source",
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
    key: "dependency",
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
    key: "scope",
    width: 90,
    render(row: any) {
      return h("span", { class: "mono" }, row.dependency.scope);
    },
  },
  {
    title: "解析路径 / 说明",
    key: "resolvedPath",
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

onMounted(reload);
</script>

<style scoped>
.runtime-deps {
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
  border: 1px solid var(--gw-border);
  color: var(--gw-text);
}
.chip b {
  font-size: 13px;
  margin: 0 2px;
}
.chip-source {
  border-color: var(--gw-success);
  color: var(--gw-success);
}
.chip-local {
  border-color: var(--gw-warning);
  color: var(--gw-warning);
}
.chip-remote {
  border-color: var(--gw-info);
  color: var(--gw-info);
}
.chip-total {
  background: var(--gw-bg-hover);
}
.truncated-hint {
  color: var(--gw-warning);
}
.main-layout {
  display: flex;
  gap: var(--gw-space-3);
  flex: 1;
  min-height: 0;
}
.project-panel {
  width: 300px;
  flex-shrink: 0;
  border: 1px solid var(--gw-border);
  border-radius: 8px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.detail-panel {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--gw-border);
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
  background: var(--gw-bg-hover);
}
.project-item.active {
  border-color: var(--gw-accent);
  background: var(--gw-bg-hover);
}
.project-name {
  font-weight: 600;
  font-size: 13px;
}
.project-path {
  font-size: 11px;
  color: var(--gw-text-dim);
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
  color: var(--gw-text-dim);
  margin-top: 2px;
}
.panel-empty {
  text-align: center;
  color: var(--gw-text-dim);
  font-size: 12px;
  padding: 24px 0;
  line-height: 1.8;
}
.inspect-box {
  border: 1px dashed var(--gw-border);
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
  gap: var(--gw-space-3);
  font-size: 12px;
  padding: 2px 0;
}
.mapping-row .path {
  color: var(--gw-accent);
}
.filter-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}
.filter-label {
  font-size: 12px;
  color: var(--gw-text-dim);
}
.filter-project {
  font-size: 12px;
  color: var(--gw-text);
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
}
.muted {
  color: var(--gw-text-dim);
}
.opt-tag {
  margin-left: 6px;
}

/* ----- R-20 树·图视图 ----- */
.tree-panel {
  width: 46%;
  flex-shrink: 0;
  border: 1px solid var(--gw-border);
  border-radius: 8px;
  padding: 8px;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.tree-scroll-wrap {
  flex: 1;
  min-height: 0;
}
.tree-list {
  height: 100%;
}
.tree-row {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 34px;
  padding-right: 8px;
  border-radius: 6px;
  cursor: pointer;
  box-sizing: border-box;
}
.tree-row:hover {
  background: var(--gw-bg-hover);
}
.tree-row.selected {
  background: var(--gw-bg-hover);
  outline: 1px solid var(--gw-accent);
}
.tree-arrow {
  width: 14px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
  font-size: 11px;
  text-align: center;
  cursor: pointer;
}
.tree-arrow.leaf {
  cursor: default;
}
.tree-check {
  flex-shrink: 0;
}
.tree-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}
.tree-dot.dot-source {
  background: var(--gw-success);
}
.tree-dot.dot-local {
  background: var(--gw-warning);
}
.tree-dot.dot-remote {
  background: var(--gw-info);
}
.tree-label {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
}
.tree-label.label-app {
  color: var(--gw-accent);
}
.tree-coords {
  font-size: 11px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.tree-source-tag {
  font-size: 11px;
  flex-shrink: 0;
}
.tree-source-tag.dot-source {
  color: var(--gw-success);
}
.tree-source-tag.dot-local {
  color: var(--gw-warning);
}
.tree-source-tag.dot-remote {
  color: var(--gw-info);
}
.scope-box {
  border: 1px dashed var(--gw-border);
  border-radius: 6px;
  padding: 8px 10px;
}
</style>
