<template>
  <div class="health-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          @change="onWorkspaceChange"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <el-button
          type="primary"
          :loading="loading"
          :disabled="!selectedWorkspaceId"
          @click="reload"
        >
          <el-icon><Refresh /></el-icon>
          重新检测
        </el-button>
      </div>
      <div class="toolbar-right">
        <span v-if="extrasLoading" class="extras-hint">
          <el-icon class="is-loading"><Loading /></el-icon>
          重项检测中（大文件 / LFS / 子模块）…
        </span>
      </div>
    </div>

    <!-- Score panel -->
    <div class="score-panel" v-loading="loading">
      <div class="score-main">
        <div class="score-value" :class="scoreTone">
          {{ score }}<span class="score-unit">%</span>
        </div>
        <div class="score-label">Workspace Health</div>
      </div>
      <div class="score-side">
        <el-progress
          :percentage="score"
          :status="progressStatus"
          :stroke-width="14"
          :striped="extrasLoading"
        />
        <div class="score-meta">
          {{ anomalousCount }} / {{ total }} 个仓库存在异常
        </div>
      </div>
    </div>

    <!-- Scoring rules (weights from health-weights.json or defaults) -->
    <el-collapse class="weights-collapse">
      <el-collapse-item
        title="评分规则：每仓库 100 分起，按异常项扣权重分（下限 0），工作区取平均；权重配置于应用数据目录 health-weights.json"
        name="weights"
      >
        <div class="weights-grid">
          <span v-for="w in weightRows" :key="w.key" class="weight-item">
            {{ w.label }} <b>-{{ w.value }}</b>
          </span>
        </div>
      </el-collapse-item>
    </el-collapse>

    <!-- Anomaly cards: click to drill down (filter the repo table) -->
    <div class="anomaly-cards" v-loading="loading">
      <div
        v-for="a in anomalyCards"
        :key="a.key"
        class="anomaly-card"
        :class="{ active: activeFilter === a.key, zero: a.count === 0 }"
        @click="toggleFilter(a.key)"
      >
        <div class="anomaly-count">{{ a.count }}</div>
        <div class="anomaly-label">{{ a.label }}</div>
      </div>
    </div>

    <!-- Repo table -->
    <div class="section">
      <div class="section-head">
        <el-checkbox v-model="onlyAnomalous">仅显示异常仓库</el-checkbox>
        <el-tag v-if="activeFilter" closable @close="activeFilter = ''">
          {{ anomalyLabel(activeFilter) }}
        </el-tag>
        <el-input
          v-model="searchQuery"
          placeholder="按名称 / 路径筛选"
          style="width: 220px; margin-left: auto"
          clearable
          :prefix-icon="Search"
        />
      </div>
      <el-table
        :data="tableRows"
        v-loading="loading"
        size="small"
        :default-sort="{ prop: 'score', order: 'ascending' }"
      >
        <el-table-column label="仓库" min-width="180">
          <template #default="{ row }">
            <div class="repo-cell">
              <span class="repo-name">{{ row.repoName }}</span>
              <span class="repo-path">{{ row.repoPath }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column prop="branch" label="分支" width="130">
          <template #default="{ row }">
            <el-tag size="small" effect="plain">{{ row.branch }}</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="异常项" min-width="240">
          <template #default="{ row }">
            <el-tag
              v-for="a in row.anomalies"
              :key="a"
              size="small"
              class="anomaly-tag"
              :type="anomalyTagType(a)"
              @click.stop="toggleFilter(a)"
            >
              {{ anomalyLabel(a) }}
            </el-tag>
            <span v-if="row.anomalies.length === 0" class="text-ok">健康</span>
          </template>
        </el-table-column>
        <el-table-column
          prop="score"
          label="评分"
          width="90"
          align="right"
          sortable
        >
          <template #default="{ row }">
            <span :class="scoreClass(row.score)">{{ row.score }}</span>
          </template>
        </el-table-column>
      </el-table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { Back, Refresh, Search, Loading } from "@element-plus/icons-vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { getWorkspaceHealth, getHealthExtras } from "@/api/health";
import type {
  HealthWeights,
  RepoHealth,
  RepoHealthExtra,
} from "@/types/health";
import { errMsg } from "@/utils/error";

interface AnomalyMeta {
  key: string;
  label: string;
  weightKey: keyof HealthWeights;
  heavy: boolean;
}

/** Anomaly metadata; keys match core/health.rs ANOMALY_* constants. */
const ANOMALIES: AnomalyMeta[] = [
  { key: "dirty", label: "有变更", weightKey: "dirty", heavy: false },
  { key: "conflict", label: "冲突", weightKey: "conflict", heavy: false },
  { key: "ahead", label: "领先", weightKey: "ahead", heavy: false },
  { key: "behind", label: "落后", weightKey: "behind", heavy: false },
  { key: "detached", label: "HEAD 游离", weightKey: "detached", heavy: false },
  {
    key: "missing_remote",
    label: "缺远程",
    weightKey: "missingRemote",
    heavy: false,
  },
  { key: "diverged", label: "分歧", weightKey: "diverged", heavy: false },
  { key: "untracked", label: "未跟踪", weightKey: "untracked", heavy: false },
  { key: "large_files", label: "大文件", weightKey: "largeFiles", heavy: true },
  { key: "lfs_error", label: "LFS 异常", weightKey: "lfsError", heavy: true },
  {
    key: "submodule_error",
    label: "子模块异常",
    weightKey: "submoduleError",
    heavy: true,
  },
];

const router = useRouter();
const workspaceStore = useWorkspaceStore();

const selectedWorkspaceId = ref<number | null>(null);
const loading = ref(false);
const extrasLoading = ref(false);
const repos = ref<RepoHealth[]>([]);
const weights = ref<HealthWeights | null>(null);
const score = ref(100);
const total = ref(0);
const anomalousCount = ref(0);
const activeFilter = ref("");
const onlyAnomalous = ref(true);
const searchQuery = ref("");

const weightRows = computed(() =>
  ANOMALIES.map((a) => ({
    key: a.key,
    label: a.label,
    value: weights.value ? weights.value[a.weightKey] : "—",
  })),
);

const anomalyCards = computed(() =>
  ANOMALIES.map((a) => ({
    key: a.key,
    label: a.label,
    count: repos.value.filter((r) => r.anomalies.includes(a.key)).length,
  })),
);

const scoreTone = computed(() => {
  if (score.value >= 90) return "tone-ok";
  if (score.value >= 70) return "tone-warn";
  return "tone-danger";
});

const progressStatus = computed(() => {
  if (score.value >= 90) return "success";
  if (score.value >= 70) return "warning";
  return "exception";
});

const tableRows = computed(() => {
  let rows = repos.value;
  if (onlyAnomalous.value) {
    rows = rows.filter((r) => r.anomalies.length > 0);
  }
  if (activeFilter.value) {
    rows = rows.filter((r) => r.anomalies.includes(activeFilter.value));
  }
  const q = searchQuery.value.trim().toLowerCase();
  if (q) {
    rows = rows.filter(
      (r) =>
        r.repoName.toLowerCase().includes(q) ||
        r.repoPath.toLowerCase().includes(q),
    );
  }
  return rows;
});

function anomalyLabel(key: string): string {
  return ANOMALIES.find((a) => a.key === key)?.label ?? key;
}

function anomalyTagType(key: string): "danger" | "warning" | "info" {
  if (key === "conflict" || key === "lfs_error" || key === "submodule_error") {
    return "danger";
  }
  if (key === "ahead" || key === "missing_remote") return "info";
  return "warning";
}

function scoreClass(s: number): string {
  if (s >= 90) return "text-ok";
  if (s >= 70) return "text-warn";
  return "text-danger";
}

function toggleFilter(key: string) {
  activeFilter.value = activeFilter.value === key ? "" : key;
}

/** Mirror of core/health.rs::score_of — used to re-score after the async
 * heavy checks land. Keep the formula in sync with the Rust side. */
function scoreOf(anomalies: string[], w: HealthWeights): number {
  const deduction = anomalies.reduce((n, a) => {
    const meta = ANOMALIES.find((m) => m.key === a);
    return n + (meta ? w[meta.weightKey] : 0);
  }, 0);
  return Math.max(0, 100 - deduction);
}

/** Merge async heavy-check results into the table and re-score (same
 * rounding as core/health.rs::aggregate_health). */
function applyExtras(extras: RepoHealthExtra[]) {
  const w = weights.value;
  if (!w) return;
  const byPath = new Map(extras.map((e) => [e.repoPath, e]));
  for (const r of repos.value) {
    const e = byPath.get(r.repoPath);
    if (!e) continue;
    if (e.largeFiles > 0 && !r.anomalies.includes("large_files")) {
      r.anomalies.push("large_files");
    }
    if (e.lfsError && !r.anomalies.includes("lfs_error")) {
      r.anomalies.push("lfs_error");
    }
    if (e.submoduleError && !r.anomalies.includes("submodule_error")) {
      r.anomalies.push("submodule_error");
    }
    r.score = scoreOf(r.anomalies, w);
  }
  const t = repos.value.length;
  if (t > 0) {
    const sum = repos.value.reduce((n, r) => n + r.score, 0);
    score.value = Math.floor((sum + t / 2) / t);
  }
  anomalousCount.value = repos.value.filter(
    (r) => r.anomalies.length > 0,
  ).length;
}

async function reload() {
  if (!selectedWorkspaceId.value) return;
  loading.value = true;
  activeFilter.value = "";
  try {
    // Phase 1: light checks from the T-02 status cache (instant).
    const health = await getWorkspaceHealth(selectedWorkspaceId.value);
    repos.value = health.repos;
    weights.value = health.weights;
    score.value = health.score;
    total.value = health.total;
    anomalousCount.value = health.anomalous;
  } catch (e) {
    ElMessage.error("健康检测失败: " + errMsg(e));
    return;
  } finally {
    loading.value = false;
  }

  // Phase 2: heavy checks (large files / LFS / submodule) — async, the page
  // stays interactive while they compute (T-19 验收: 打开不阻塞 UI).
  const paths = repos.value.map((r) => r.repoPath);
  if (paths.length === 0) return;
  extrasLoading.value = true;
  try {
    const extras = await getHealthExtras(paths);
    applyExtras(extras);
  } catch (e) {
    ElMessage.warning("重项检测失败（大文件/LFS/子模块）: " + errMsg(e));
  } finally {
    extrasLoading.value = false;
  }
}

function onWorkspaceChange(id: number) {
  selectedWorkspaceId.value = id;
  const ws = workspaceStore.workspaces.find((w) => w.id === id);
  if (ws) workspaceStore.selectWorkspace(ws);
  reload();
}

function goBack() {
  router.push({ name: "dashboard" });
}

onMounted(async () => {
  await workspaceStore.loadWorkspaces();
  if (workspaceStore.currentWorkspace) {
    selectedWorkspaceId.value = workspaceStore.currentWorkspace.id;
    await reload();
  }
});
</script>

<style scoped>
.health-view {
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

.extras-hint {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.score-panel {
  display: flex;
  align-items: center;
  gap: 28px;
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 18px 22px;
}

.score-value {
  font-size: 52px;
  font-weight: 700;
  line-height: 1;
}

.score-unit {
  font-size: 24px;
}

.score-label {
  margin-top: 6px;
  font-size: 13px;
  color: var(--el-text-color-secondary);
}

.score-side {
  flex: 1;
}

.score-meta {
  margin-top: 8px;
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.tone-ok {
  color: var(--el-color-success);
}

.tone-warn {
  color: var(--el-color-warning);
}

.tone-danger {
  color: var(--el-color-danger);
}

.weights-collapse {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 0 12px;
}

.weights-grid {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 18px;
  padding-bottom: 8px;
}

.weight-item {
  font-size: 12px;
  color: var(--el-text-color-regular);
}

.anomaly-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
  gap: 8px;
}

.anomaly-card {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 10px 12px;
  text-align: center;
  cursor: pointer;
}

.anomaly-card:hover {
  border-color: var(--el-color-primary);
}

.anomaly-card.active {
  border-color: var(--el-color-primary);
  background: var(--el-color-primary-light-9);
}

.anomaly-card.zero .anomaly-count {
  color: var(--el-text-color-secondary);
}

.anomaly-count {
  font-size: 24px;
  font-weight: 600;
}

.anomaly-label {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.section {
  border: 1px solid var(--el-border-color);
  border-radius: 8px;
  padding: 12px 14px;
}

.section-head {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
}

.repo-cell {
  display: flex;
  flex-direction: column;
}

.repo-name {
  font-weight: 500;
}

.repo-path {
  font-size: 11px;
  color: var(--el-text-color-secondary);
}

.anomaly-tag {
  margin-right: 4px;
  cursor: pointer;
}

.text-ok {
  color: var(--el-color-success);
}

.text-warn {
  color: var(--el-color-warning);
  font-weight: 600;
}

.text-danger {
  color: var(--el-color-danger);
  font-weight: 600;
}
</style>
