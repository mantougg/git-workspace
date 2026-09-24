<template>
  <div class="commit-graph">
    <div v-if="rows.length === 0 && !loading" class="empty-graph">
      <n-empty description="暂无提交记录" />
    </div>
    <template v-else>
      <!-- GF-11：定高虚拟滚动（ROW_H=30 固定行高，T-04 前端渲染预算）——
           加载再多提交，DOM 行数恒定。追加翻页不清空滚动位置
           （resetScrollOnItemsChange=false，整页重载由宿主重挂载复位）。 -->
      <div class="graph-scroll">
        <VirtualList
          :items="rows"
          :item-height="ROW_H"
          :reset-scroll-on-items-change="false"
          class="graph-list"
        >
          <template #row="{ item: row }">
            <div
              class="commit-row"
              :class="{ 'selected': selectedOid === row.commit.oid }"
              @click="selectCommit(row.commit)"
              @contextmenu.stop.prevent="onRowContextmenu($event, row.commit)"
            >
              <!-- SVG lane graph -->
              <svg
                :width="svgWidth"
                :height="ROW_H"
                class="graph-svg"
              >
                <!-- Vertical lane lines -->
                <line
                  v-for="lane in row.activeLanes"
                  :key="'v' + lane"
                  :x1="lx(lane)"
                  :y1="0"
                  :x2="lx(lane)"
                  :y2="ROW_H"
                  :stroke="laneColor(lane)"
                  stroke-width="2"
                  opacity="0.45"
                />
                <!-- Merge-in lines (multiple lanes converging to this commit) -->
                <path
                  v-for="m in row.mergeLanes"
                  :key="'m' + m"
                  :d="`M ${lx(m)} 0 L ${lx(row.lane)} ${CY}`"
                  :stroke="laneColor(m)"
                  stroke-width="2"
                  fill="none"
                  opacity="0.75"
                />
                <!-- Parent lines (this commit branching to its parents) -->
                <path
                  v-for="p in row.parentLanes"
                  :key="'p' + p.lane"
                  :d="`M ${lx(row.lane)} ${CY} L ${lx(p.lane)} ${ROW_H}`"
                  :stroke="laneColor(p.lane)"
                  stroke-width="2"
                  fill="none"
                  opacity="0.75"
                />
                <!-- Commit dot -->
                <circle
                  :cx="lx(row.lane)"
                  :cy="CY"
                  r="5"
                  :fill="isMerge(row) ? '#a855f7' : 'var(--gw-accent)'"
                  stroke="var(--gw-bg-panel)"
                  stroke-width="1.5"
                />
              </svg>

              <!-- Commit hash -->
              <span class="commit-hash">{{ row.commit.shortOid }}</span>

              <!-- Refs (branches/tags) -->
              <div class="commit-refs" v-if="row.commit.refs.length > 0">
                <n-tag
                  v-for="ref in row.commit.refs"
                  :key="ref"
                  :type="refType(ref)"
                  size="small"
                  :bordered="false"
                >
                  {{ ref }}
                </n-tag>
              </div>

              <!-- Commit message -->
              <span class="commit-message">{{ commitMessage(row.commit) }}</span>

              <!-- Author and time -->
              <span class="commit-meta">
                <span class="commit-author">{{ row.commit.author }}</span>
                <span class="commit-time">{{ formatTime(row.commit.time) }}</span>
              </span>

              <!-- Row actions (T-13 history operations) -->
              <n-dropdown trigger="click" :options="actionOptions" @select="(key: string) => onAction(key, row.commit)">
                <n-button size="small" text class="row-action-btn" @click.stop>
                  <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
                </n-button>
              </n-dropdown>
            </div>
          </template>
        </VirtualList>
      </div>
      <div v-if="loading" class="loading-more">
        <n-spin :show="true" :size="14" />
        加载中...
      </div>
      <div
        v-else-if="rows.length > 0 && hasMore"
        class="load-more"
        @click="$emit('load-more')"
      >
        加载更多…（已加载 {{ commits.length }} 条）
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { EllipsisVerticalOutline } from "@vicons/ionicons5";
import type { CommitInfo } from "@/types/graph";
import VirtualList from "@/components/common/VirtualList.vue";

const props = defineProps<{
  commits: CommitInfo[];
  loading?: boolean;
  hasMore?: boolean;
}>();

const emit = defineEmits<{
  (e: "select", commit: CommitInfo): void;
  (e: "load-more"): void;
  /** T-13 history operations: "cherry-pick" | "revert" | "reset". */
  (e: "action", action: string, commit: CommitInfo): void;
  /** D-13：提交节点右键（宿主视图渲染 ContextMenu） */
  (e: "contextmenu", commit: CommitInfo, x: number, y: number): void;
}>();

const ROW_H = 30;
const CY = ROW_H / 2;
const LANE_W = 16;

const LANE_COLORS = [
  "var(--gw-accent)",
  "var(--gw-success)",
  "var(--gw-warning)",
  "var(--gw-danger)",
  "var(--gw-text-dim)",
  "var(--gw-info)",
  "var(--gw-warning)",
  "var(--gw-accent)",
  "var(--gw-success)",
  "var(--gw-danger)",
];

const actionOptions = [
  { label: "Cherry-pick", key: "cherry-pick" },
  { label: "Revert", key: "revert" },
  { type: "divider", key: "d1" },
  { label: "Reset 到此处…", key: "reset" },
];

interface ParentLane {
  oid: string;
  lane: number;
}

interface Row {
  commit: CommitInfo;
  /** Lane column of this commit's dot. */
  lane: number;
  /** Other lanes that converge into this commit (merge commits). */
  mergeLanes: number[];
  /** Lanes of this commit's parents (next row positions). */
  parentLanes: ParentLane[];
  /** Columns with activity in this row. */
  activeLanes: number[];
}

const selectedOid = ref<string | null>(null);

/** Classic lane-based graph layout over topological commits (newest first). */
function layout(commits: CommitInfo[]): Row[] {
  const lanes: (string | null)[] = [];
  const rows: Row[] = [];

  for (const c of commits) {
    // 1. Find lanes whose tail is this commit.
    const pointing: number[] = [];
    lanes.forEach((oid, idx) => {
      if (oid === c.oid) pointing.push(idx);
    });

    let myLane: number;
    let mergeLanes: number[] = [];
    if (pointing.length > 0) {
      myLane = pointing[0];
      mergeLanes = pointing.slice(1);
      for (const m of pointing) lanes[m] = null;
    } else {
      // New branch: reuse a free lane or allocate a new one.
      myLane = lanes.indexOf(null);
      if (myLane === -1) {
        myLane = lanes.length;
        lanes.push(null);
      }
    }

    // 2. Assign parents to lanes: first parent continues this lane,
    //    remaining parents get free/new lanes.
    const parentLanes: ParentLane[] = [];
    c.parents.forEach((p, i) => {
      let lane: number;
      if (i === 0) {
        lane = myLane;
      } else {
        lane = lanes.indexOf(null);
        if (lane === -1) {
          lane = lanes.length;
          lanes.push(null);
        }
      }
      lanes[lane] = p;
      parentLanes.push({ oid: p, lane });
    });

    const activeLanes = Array.from(
      new Set([myLane, ...mergeLanes, ...parentLanes.map((p) => p.lane)]),
    );
    rows.push({ commit: c, lane: myLane, mergeLanes, parentLanes, activeLanes });
  }

  return rows;
}

const rows = computed<Row[]>(() => layout(props.commits));

const maxLanes = computed(() => {
  let max = 1;
  for (const r of rows.value) {
    for (const l of r.activeLanes) {
      if (l + 1 > max) max = l + 1;
    }
  }
  return max;
});

const svgWidth = computed(() => maxLanes.value * LANE_W);

function lx(lane: number): number {
  return lane * LANE_W + LANE_W / 2;
}

function laneColor(lane: number): string {
  return LANE_COLORS[lane % LANE_COLORS.length];
}

function isMerge(row: Row): boolean {
  return row.commit.parents.length > 1 || row.mergeLanes.length > 0;
}

function selectCommit(commit: CommitInfo) {
  selectedOid.value = commit.oid;
  emit("select", commit);
}

function onAction(key: string, commit: CommitInfo) {
  emit("action", key, commit);
}

function onRowContextmenu(e: MouseEvent, commit: CommitInfo) {
  selectedOid.value = commit.oid;
  emit("contextmenu", commit, e.clientX, e.clientY);
}

function commitMessage(commit: CommitInfo): string {
  const firstLine = commit.message.split("\n")[0];
  return firstLine.length > 80 ? firstLine.slice(0, 80) + "..." : firstLine;
}

function refType(ref: string): "success" | "warning" | "info" {
  if (ref.startsWith("origin/") || ref.includes("/")) return "warning";
  if (ref === "HEAD") return "info";
  return "success";
}

function formatTime(time: string): string {
  // Simple formatting - show date part only
  const parts = time.split(" ");
  return parts[0] || time;
}
</script>

<style scoped>
/* GF-11：虚拟滚动容器链——.commit-graph 定高列向 flex，.graph-scroll 承载
   VirtualList（其根 .virtual-list 自带 height:100% + overflow:auto）。
   flex: 1 1 auto 而非 1 1 0%：宿主高度链断裂时（如侧栏预览面板）
   退化为内容高度全量渲染，与改造前一致，不会出现 0 高空白。 */
.commit-graph {
  height: 100%;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.graph-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow: hidden;
}

/* 提交行是固定栅格 + ellipsis（message 自适应截断），覆盖 VirtualList 默认的
   max-content 宽度——否则长 message 会把行撑出横向滚动条。 */
.graph-list :deep(.virtual-list-spacer),
.graph-list :deep(.virtual-list-window) {
  width: 100%;
}

.commit-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 0 12px;
  border-bottom: 1px solid var(--gw-border);
  cursor: pointer;
  font-size: 13px;
  /* VirtualList 行包裹层已固定 ROW_H，这里填满即可 */
  height: 100%;
  overflow: hidden;
}

.commit-row:hover {
  background: var(--gw-bg-hover);
}

.commit-row.selected {
  background: var(--gw-bg-hover);
}

.graph-svg {
  flex-shrink: 0;
  display: block;
}

.commit-hash {
  color: var(--gw-text-dim);
  font-family: monospace;
  font-size: 12px;
  flex-shrink: 0;
}

.commit-refs {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}

.commit-message {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-meta {
  display: flex;
  gap: var(--gw-space-2);
  flex-shrink: 0;
  font-size: 12px;
  color: var(--gw-text-dim);
}

.commit-author {
  font-weight: 500;
}

.loading-more,
.load-more {
  text-align: center;
  padding: 8px;
  color: var(--gw-text-dim);
  font-size: 13px;
  /* 列向 flex 子项：不参与压缩，常驻虚拟列表下方 */
  flex-shrink: 0;
}

.load-more {
  cursor: pointer;
}

.load-more:hover {
  color: var(--gw-accent);
}

.empty-graph {
  padding-top: 40px;
}
</style>
