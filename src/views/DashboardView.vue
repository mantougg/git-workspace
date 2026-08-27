<template>
  <div class="dashboard">
    <!-- Top toolbar（D-05：导航按钮移除，仅保留操作类） -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button
          type="primary"
          :loading="repoStore.scanning"
          :disabled="!currentWorkspaceId"
          @click="handleScan"
        >
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          扫描仓库
        </n-button>
        <n-button
          :loading="repoStore.loading"
          :disabled="!currentWorkspaceId"
          @click="reload"
        >
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
      </div>
    </div>

    <!-- Stat cards (T-18): aggregation is a pure O(n) computed over the
         T-02 status cache data already fetched by list_repositories. -->
    <n-spin :show="repoStore.loading">
      <div class="cards">
        <div
          v-for="card in cards"
          :key="card.key"
          class="stat-card"
          :class="[`tone-${card.tone}`, { clickable: card.jumpable }]"
          @click="openCard(card)"
        >
          <div class="stat-label">{{ card.label }}</div>
          <div class="stat-value">{{ card.value }}</div>
          <div class="stat-sub">{{ card.sub }}</div>
        </div>
      </div>
    </n-spin>

    <!-- Status distribution -->
    <div class="section">
      <div class="section-title">状态分布</div>
      <div class="dist-bar">
        <template v-if="total > 0">
          <div
            v-for="seg in distribution"
            :key="seg.key"
            class="dist-seg"
            :style="{ width: seg.pct + '%', background: seg.color }"
            :title="`${seg.label} ${seg.count}（${seg.pct.toFixed(1)}%）`"
          ></div>
        </template>
        <div v-else class="dist-empty">暂无仓库，请先扫描</div>
      </div>
      <div v-if="total > 0" class="dist-legend">
        <span v-for="seg in distribution" :key="seg.key" class="legend-item">
          <i class="legend-dot" :style="{ background: seg.color }"></i>
          {{ seg.label }} {{ seg.count }}（{{ seg.pct.toFixed(1) }}%）
        </span>
      </div>
    </div>

    <!-- Commit heatmap (F-01b)：当前用户在当前工作区所有仓库的提交热力图 -->
    <div v-if="currentWorkspaceId" class="section">
      <div class="section-title">
        提交热力图
        <span v-if="heatmap.identity" class="section-sub">{{ heatmap.identity }}</span>
      </div>
      <n-spin :show="heatmapLoading">
        <CommitHeatmap v-if="heatmap.days.length > 0" :days="heatmap.days" />
        <div v-else class="section-empty">
          {{ heatmap.identity ? "近一年没有匹配到你的提交" : "未配置 git user.email / user.name，无法识别你的提交" }}
        </div>
      </n-spin>
    </div>

    <!-- Health summary (F-01c)：健康检查前置到首页，轻项走缓存即时返回 -->
    <div v-if="currentWorkspaceId" class="section">
      <div class="section-title">
        健康检查
        <n-button size="tiny" text type="primary" @click="goHealth">查看详情</n-button>
      </div>
      <n-spin :show="healthLoading">
        <div v-if="health" class="health-summary">
          <span class="health-score" :class="healthScoreClass">{{ health.score }}%</span>
          <span class="health-meta">
            {{ health.anomalous > 0 ? `${health.anomalous} / ${health.total} 个仓库存在异常` : `${health.total} 个仓库全部健康` }}
          </span>
          <n-button size="small" :loading="healthLoading" @click="loadInsights">
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            重新检测
          </n-button>
        </div>
        <div v-else-if="!healthLoading" class="section-empty">暂无健康数据</div>
      </n-spin>
    </div>

    <!-- Runtime apps (F-01d)：当前工作区已创建的应用 -->
    <div v-if="currentWorkspaceId" class="section">
      <div class="section-title">
        我的应用
        <n-button size="tiny" text type="primary" @click="router.push({ name: 'runtime-dashboard' })">
          进入 Runtime
        </n-button>
      </div>
      <n-spin :show="appsLoading">
        <div v-if="apps.length > 0" class="app-cards">
          <div
            v-for="app in apps"
            :key="app.id"
            class="app-card"
            @click="router.push({ name: 'runtime-dashboard' })"
          >
            <div class="app-name">{{ app.name }}</div>
            <div class="app-meta">
              <n-tag size="small" :bordered="false">{{ app.project.split(/[\\/]/).pop() }}</n-tag>
              <n-tag v-if="app.profile" size="small" type="info" :bordered="false">{{ app.profile }}</n-tag>
              <n-tag v-if="app.jdk" size="small" type="warning" :bordered="false">{{ app.jdk }}</n-tag>
            </div>
          </div>
        </div>
        <div v-else-if="!appsLoading" class="section-empty">
          还没有创建应用
          <n-button size="tiny" text type="primary" @click="router.push({ name: 'runtime-app-wizard' })">
            去创建
          </n-button>
        </div>
      </n-spin>
    </div>

    <!-- Group breakdown -->
    <div v-if="groupRows.length > 0" class="section">
      <div class="section-title">分组视图</div>
      <n-data-table
        :columns="groupColumns"
        :data="groupRows"
        size="small"
        :row-props="groupRowProps"
        class="group-table"
      />
    </div>

    <!-- Quick actions: jump into the T-20 batch-ops view with a prefilled
         selection; the user confirms execution there (Safety First). -->
    <div class="section">
      <div class="section-title">快捷操作</div>
      <div class="actions">
        <n-button :disabled="total === 0" @click="quickAction('fetch')">
          <template #icon><n-icon><DownloadOutline /></n-icon></template>
          Fetch All
        </n-button>
        <n-button :disabled="total === 0" @click="quickAction('pull')">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          Pull Clean
        </n-button>
        <n-button :disabled="total === 0" @click="quickAction('push')">
          <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
          Push
        </n-button>
        <n-button :disabled="total === 0" @click="quickAction('commit')">
          <template #icon><n-icon><CreateOutline /></n-icon></template>
          Commit
        </n-button>
        <n-button :disabled="total === 0" @click="quickAction('branch-create')">
          <template #icon><n-icon><AddCircleOutline /></n-icon></template>
          Create Branch
        </n-button>
        <n-tooltip trigger="hover">
          <template #trigger>
            <span>
              <n-button disabled>
                <template #icon><n-icon><ArchiveOutline /></n-icon></template>
                Stash
              </n-button>
            </span>
          </template>
          批量 Stash 将随 T-21（Workspace Stash & Branch）提供
        </n-tooltip>
      </div>
      <div class="actions-hint">
        跳转到批量操作视图并预填选择，在那里确认后执行
      </div>
    </div>

  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref, watch } from "vue";
import { useRouter } from "vue-router";

import {
  RefreshOutline,
  DownloadOutline,
  CloudUploadOutline,
  CreateOutline,
  AddCircleOutline,
  ArchiveOutline,
} from "@vicons/ionicons5";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { useRepositories } from "@/composables/useRepositories";
import { listGroups } from "@/api/group";
import { startWatcher } from "@/api/git_ops";
import { getCommitHeatmap } from "@/api/heatmap";
import { getWorkspaceHealth } from "@/api/health";
import { listRuntimeConfigs } from "@/api/runtime";
import type { RepoGroup } from "@/types/group";
import type { RepoStatus } from "@/types/repository";
import type { CommitHeatmap as CommitHeatmapData } from "@/types/heatmap";
import type { WorkspaceHealth } from "@/types/health";
import type { RuntimeConfigSummary } from "@/types/runtime";
import CommitHeatmap from "@/components/repo/CommitHeatmap.vue";
import { errMsg } from "@/utils/error";
import { useMessage } from "naive-ui";

interface StatCard {
  key: string;
  label: string;
  value: number;
  sub: string;
  tone: string;
  /** T-20 selector token used as the jump prefill (empty = no jump). */
  selector: string;
  jumpable: boolean;
}

interface DistSegment {
  key: string;
  label: string;
  count: number;
  pct: number;
  color: string;
}

const router = useRouter();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();
const message = useMessage();

// Live status updates from the file watcher (T-06 batch events): the store
// is patched in place, so every computed below stays current (< 500 ms,
// bounded by the watcher aggregation window).
useRepositories();

// D-05：响应全局工作区 store，不再维护本地 currentWorkspaceId
const currentWorkspaceId = computed(() => workspaceStore.currentWorkspace?.id ?? null);
const groups = ref<RepoGroup[]>([]);

// F-01：首页数据面板（热力图 / 健康摘要 / 我的应用）。
const heatmap = ref<CommitHeatmapData>({ identity: null, days: [] });
const heatmapLoading = ref(false);
const health = ref<WorkspaceHealth | null>(null);
const healthLoading = ref(false);
const apps = ref<RuntimeConfigSummary[]>([]);
const appsLoading = ref(false);

const healthScoreClass = computed(() => {
  const score = health.value?.score ?? 100;
  if (score >= 80) return "score-good";
  if (score >= 60) return "score-warn";
  return "score-bad";
});

// Group table columns for n-data-table
const groupColumns = [
  {
    title: "分组",
    key: "name",
    minWidth: 160,
  },
  {
    title: "仓库",
    key: "total",
    width: 70,
    align: "right" as const,
  },
  {
    title: "有变更",
    key: "dirty",
    width: 80,
    align: "right" as const,
    render(row: { dirty: number }) {
      return h("span", { class: row.dirty > 0 ? "num-warn" : "" }, row.dirty);
    },
  },
  {
    title: "冲突",
    key: "conflict",
    width: 70,
    align: "right" as const,
    render(row: { conflict: number }) {
      return h(
        "span",
        { class: row.conflict > 0 ? "num-danger" : "" },
        row.conflict
      );
    },
  },
  {
    title: "↑ Ahead",
    key: "ahead",
    width: 90,
    align: "right" as const,
  },
  {
    title: "↓ Behind",
    key: "behind",
    width: 90,
    align: "right" as const,
  },
];

// Row click handler for group table
const groupRowProps = (row: { name: string; id: number | null }) => ({
  style: "cursor: pointer",
  onClick: () => onGroupClick(row),
});

// --- Aggregation (O(n) over already-fetched statuses; no extra IPC) ---

const total = computed(() => repoStore.repositories.length);
const statuses = computed(() =>
  repoStore.repositories
    .map((r) => r.status)
    .filter((s): s is RepoStatus => s !== null),
);

const fileChanges = (s: RepoStatus) =>
  s.modified + s.added + s.deleted + s.staged + s.untracked;

const countWhere = (pred: (s: RepoStatus) => boolean) =>
  statuses.value.filter(pred).length;

const sumWhere = (
  pred: (s: RepoStatus) => boolean,
  pick: (s: RepoStatus) => number,
) => statuses.value.filter(pred).reduce((n, s) => n + pick(s), 0);

const cleanCount = computed(() => countWhere((s) => s.isClean));
const modifiedCount = computed(() =>
  countWhere((s) => s.modified + s.added + s.deleted + s.staged > 0),
);
const untrackedCount = computed(() => countWhere((s) => s.untracked > 0));
const conflictCount = computed(() => countWhere((s) => s.conflicted > 0));
const aheadCount = computed(() => countWhere((s) => s.ahead > 0));
const behindCount = computed(() => countWhere((s) => s.behind > 0));
const detachedCount = computed(() => countWhere((s) => s.isDetached));

const cards = computed<StatCard[]>(() => [
  {
    key: "repos",
    label: "仓库",
    value: total.value,
    sub: `${statuses.value.length} 已加载状态`,
    tone: "plain",
    selector: "",
    jumpable: true,
  },
  {
    key: "clean",
    label: "干净",
    value: cleanCount.value,
    sub: total.value > 0 ? pctText(cleanCount.value) : "—",
    tone: "ok",
    selector: "@status:clean",
    jumpable: true,
  },
  {
    key: "modified",
    label: "有变更",
    value: modifiedCount.value,
    sub: `${sumWhere((s) => s.modified + s.added + s.deleted + s.staged > 0, (s) => s.modified + s.added + s.deleted + s.staged)} 个跟踪文件`,
    tone: "warn",
    selector: "@status:dirty",
    jumpable: true,
  },
  {
    key: "untracked",
    label: "未跟踪",
    value: untrackedCount.value,
    sub: `${sumWhere((s) => s.untracked > 0, (s) => s.untracked)} 个文件`,
    tone: "warn",
    selector: "",
    jumpable: false,
  },
  {
    key: "conflict",
    label: "冲突",
    value: conflictCount.value,
    sub: `${sumWhere((s) => s.conflicted > 0, (s) => s.conflicted)} 个文件`,
    tone: "danger",
    selector: "@status:conflict",
    jumpable: true,
  },
  {
    key: "ahead",
    label: "Ahead",
    value: aheadCount.value,
    sub: `↑ ${sumWhere((s) => s.ahead > 0, (s) => s.ahead)} 个提交`,
    tone: "info",
    selector: "@status:ahead",
    jumpable: true,
  },
  {
    key: "behind",
    label: "Behind",
    value: behindCount.value,
    sub: `↓ ${sumWhere((s) => s.behind > 0, (s) => s.behind)} 个提交`,
    tone: "info",
    selector: "@status:behind",
    jumpable: true,
  },
  {
    key: "detached",
    label: "Detached HEAD",
    value: detachedCount.value,
    sub: "HEAD 游离",
    tone: "muted",
    selector: "@status:detached",
    jumpable: true,
  },
]);

function pctText(n: number): string {
  return `${((n / Math.max(total.value, 1)) * 100).toFixed(0)}%`;
}

// Distribution bar buckets are mutually exclusive and sum to the total:
// conflict > file-dirty > sync-only (ahead/behind) > clean.
const distribution = computed<DistSegment[]>(() => {
  const t = total.value;
  if (t === 0) return [];
  const conflict = conflictCount.value;
  const dirty = countWhere(
    (s) => s.conflicted === 0 && fileChanges(s) > 0,
  );
  const syncOnly = countWhere(
    (s) =>
      s.conflicted === 0 &&
      fileChanges(s) === 0 &&
      (s.ahead > 0 || s.behind > 0),
  );
  const clean = t - conflict - dirty - syncOnly;
  const toSeg = (
    key: string,
    label: string,
    count: number,
    color: string,
  ): DistSegment => ({
    key,
    label,
    count,
    pct: (count / t) * 100,
    color,
  });
  return [
    toSeg("clean", "干净", clean, "var(--el-color-success)"),
    toSeg("dirty", "有变更", dirty, "var(--el-color-warning)"),
    toSeg("conflict", "冲突", conflict, "var(--el-color-danger)"),
    toSeg("sync", "仅领先/落后", syncOnly, "var(--el-color-info)"),
  ].filter((s) => s.count > 0);
});

// --- Group breakdown ---

const groupRows = computed(() => {
  const rows = groups.value.map((g) => {
    const members = repoStore.repositories.filter(
      (r) => r.repository.groupId === g.id,
    );
    return {
      id: g.id as number | null,
      name: g.name,
      ...aggregate(members.map((m) => m.status)),
    };
  });
  const ungrouped = repoStore.repositories.filter(
    (r) => r.repository.groupId === null,
  );
  if (ungrouped.length > 0 && groups.value.length > 0) {
    rows.push({
      id: null,
      name: "（未分组）",
      ...aggregate(ungrouped.map((m) => m.status)),
    });
  }
  return rows;
});

function aggregate(list: (RepoStatus | null)[]) {
  const ss = list.filter((s): s is RepoStatus => s !== null);
  return {
    total: list.length,
    dirty: ss.filter((s) => !s.isClean).length,
    conflict: ss.filter((s) => s.conflicted > 0).length,
    ahead: ss.reduce((n, s) => n + s.ahead, 0),
    behind: ss.reduce((n, s) => n + s.behind, 0),
  };
}

// --- Navigation (jump to T-20 batch ops with prefilled selection) ---

function goHealth() {
  router.push({ name: "health" });
}

function goChanges(selector?: string, action?: string) {
  router.push({
    name: "changes",
    query: {
      ...(selector ? { selector } : {}),
      ...(action ? { action } : {}),
    },
  });
}

function openCard(card: StatCard) {
  if (!card.jumpable) return;
  goChanges(card.selector || undefined);
}

function onGroupClick(row: { name: string; id: number | null }) {
  if (row.id === null) return;
  goChanges(`@group:${row.name}`);
}

function quickAction(action: string) {
  const selector = {
    pull: "@status:clean",
    push: "@status:ahead",
    commit: "@status:dirty",
  }[action];
  goChanges(selector, action);
}

// --- Data loading ---

/** F-01：热力图 / 健康摘要 / 我的应用，并行加载、互不阻塞。 */
async function loadInsights() {
  const wsId = currentWorkspaceId.value;
  if (!wsId) return;
  heatmapLoading.value = true;
  healthLoading.value = true;
  appsLoading.value = true;
  const [heatmapRes, healthRes, appsRes] = await Promise.allSettled([
    getCommitHeatmap(wsId),
    getWorkspaceHealth(wsId),
    listRuntimeConfigs(wsId),
  ]);
  if (heatmapRes.status === "fulfilled") heatmap.value = heatmapRes.value;
  else console.error("Failed to load heatmap:", heatmapRes.reason);
  if (healthRes.status === "fulfilled") health.value = healthRes.value;
  else console.error("Failed to load health:", healthRes.reason);
  if (appsRes.status === "fulfilled") apps.value = appsRes.value;
  else console.error("Failed to load runtime apps:", appsRes.reason);
  heatmapLoading.value = false;
  healthLoading.value = false;
  appsLoading.value = false;
}

async function reload() {
  if (!currentWorkspaceId.value) return;
  await repoStore.loadRepositories(currentWorkspaceId.value);
  try {
    groups.value = await listGroups(currentWorkspaceId.value);
  } catch (e) {
    console.error("Failed to load groups:", e);
  }
  loadInsights();
  // Keep the watcher mounted on the workspace repo set so the cards update
  // live even before the user opens the batch-ops view (idempotent, delta-
  // based on the backend).
  const paths = repoStore.repositories.map((r) => r.repository.path);
  if (paths.length > 0) {
    try {
      await startWatcher(paths);
    } catch (e) {
      console.error("Failed to start watcher:", e);
    }
  }
}

async function handleScan() {
  if (!currentWorkspaceId.value) return;
  try {
    await repoStore.scanRepositories(currentWorkspaceId.value);
    message.success(`发现 ${repoStore.totalCount} 个仓库`);
    await reload();
  } catch (e) {
    message.error("扫描失败: " + errMsg(e));
  }
}

// D-05：监听全局工作区变化，自动重新加载
watch(() => workspaceStore.currentWorkspace, (ws) => {
  if (ws) reload();
});

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    await reload();
  }
});
</script>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: 12px 16px;
  gap: 12px;
  overflow-y: auto;
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
}

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 10px;
  min-height: 96px;
}

.stat-card {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px 14px;
  background: var(--el-bg-color);
}

.stat-card.clickable {
  cursor: pointer;
}

.stat-card.clickable:hover {
  border-color: var(--el-color-primary);
  box-shadow: var(--el-box-shadow-lighter);
}

.stat-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.stat-value {
  font-size: 28px;
  font-weight: 600;
  line-height: 1.3;
}

.stat-sub {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.tone-ok .stat-value {
  color: var(--el-color-success);
}

.tone-warn .stat-value {
  color: var(--el-color-warning);
}

.tone-danger .stat-value {
  color: var(--el-color-danger);
}

.tone-info .stat-value {
  color: var(--el-color-primary);
}

.tone-muted .stat-value {
  color: var(--el-text-color-secondary);
}

.section {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px 14px;
}

.section-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 10px;
}

.dist-bar {
  display: flex;
  height: 14px;
  border-radius: 7px;
  overflow: hidden;
  background: var(--el-fill-color-light);
}

.dist-seg {
  height: 100%;
  transition: width 0.3s;
}

.dist-empty {
  width: 100%;
  text-align: center;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  line-height: 14px;
}

.dist-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
  margin-top: 8px;
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--el-text-color-regular);
}

.legend-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.group-table {
  cursor: pointer;
}

.num-warn {
  color: var(--el-color-warning);
  font-weight: 600;
}

.num-danger {
  color: var(--el-color-danger);
  font-weight: 600;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.actions .n-button + .n-button,
.actions span .n-button {
  margin-left: 0;
}

.actions-hint {
  margin-top: 8px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

/* F-01：热力图 / 健康摘要 / 我的应用 */
.section-sub {
  margin-left: 8px;
  font-size: 12px;
  font-weight: 400;
  color: #909399;
}

.section-title .n-button {
  margin-left: 8px;
}

.section-empty {
  font-size: 12px;
  color: #909399;
  padding: 8px 0;
}

.health-summary {
  display: flex;
  align-items: center;
  gap: 14px;
}

.health-score {
  font-size: 28px;
  font-weight: 700;
}

.health-score.score-good {
  color: #18a058;
}

.health-score.score-warn {
  color: #f0a020;
}

.health-score.score-bad {
  color: #d03050;
}

.health-meta {
  flex: 1;
  font-size: 13px;
  color: #606266;
}

.app-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
  gap: 10px;
}

.app-card {
  border: 1px solid #ebeef5;
  border-radius: 6px;
  padding: 10px 12px;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 6px;
  transition: box-shadow 0.15s;
}

.app-card:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}

.app-name {
  font-size: 14px;
  font-weight: 600;
}

.app-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
</style>
