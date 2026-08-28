<template>
  <div class="commit-heatmap">
    <!-- F-21：横轴 = 月份标签（按列首格月份变化定位），纵轴 = 星期标签 -->
    <div class="heatmap-months">
      <span
        v-for="m in monthLabels"
        :key="m.index"
        class="month-label"
        :style="{ left: m.left }"
      >{{ m.text }}</span>
    </div>
    <div class="heatmap-row">
      <div class="heatmap-y-axis">
        <span v-for="(w, i) in weekdayLabels" :key="i" class="weekday-label">{{ w }}</span>
      </div>
      <div class="heatmap-scroll">
        <div
          class="heatmap-grid"
          @mousemove="onCellHover"
          @mouseleave="tip.show = false"
        >
          <div
            v-for="(cell, i) in cells"
            :key="i"
            class="heatmap-cell"
            :class="`level-${cell.level}`"
            :data-tip="cell.date ? `${cell.date}：${cell.count} 次提交` : undefined"
          ></div>
        </div>
      </div>
    </div>
    <div class="heatmap-legend">
      <span class="legend-text">少</span>
      <span v-for="level in [0, 1, 2, 3, 4]" :key="level" class="heatmap-cell legend-cell" :class="`level-${level}`"></span>
      <span class="legend-text">多</span>
    </div>
    <!-- F-19：自定义悬浮提示（原生 title 在 Tauri WebView 中不可靠）；
         单元素复用 + 网格事件委托，格子不各挂 tooltip 组件 -->
    <div
      v-if="tip.show"
      class="heatmap-tip"
      :style="{ left: tip.x + 'px', top: tip.y + 'px' }"
    >
      {{ tip.text }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive } from "vue";

interface HeatmapDay {
  date: string;
  count: number;
}

interface Cell {
  date: string;
  count: number;
  level: number;
}

const props = defineProps<{
  days: HeatmapDay[];
}>();

const countByDate = computed(() => {
  const map = new Map<string, number>();
  for (const d of props.days) map.set(d.date, d.count);
  return map;
});

function levelOf(count: number, max: number): number {
  if (count === 0) return 0;
  const ratio = count / max;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.5) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

function formatDate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * 最近约一年的单元格流（grid-auto-flow: column，一列 = 一周）。
 * 起点对齐到周日，末尾补齐当前周剩余的空位。
 */
const cells = computed<Cell[]>(() => {
  const max = Math.max(0, ...props.days.map((d) => d.count));
  const today = new Date();
  const start = new Date(today);
  start.setDate(start.getDate() - 363);
  // 对齐到周日（getDay: 0=周日）。
  start.setDate(start.getDate() - start.getDay());

  const result: Cell[] = [];
  const cursor = new Date(start);
  while (true) {
    const inRange = cursor <= today;
    const date = formatDate(cursor);
    const count = inRange ? countByDate.value.get(date) ?? 0 : 0;
    result.push({
      date: inRange ? date : "",
      count,
      level: inRange ? levelOf(count, max) : 0,
    });
    cursor.setDate(cursor.getDate() + 1);
    if (cursor > today && cursor.getDay() === 0) break;
  }
  return result;
});

// --- F-21：横纵坐标 ---
// 纵轴星期标签：行 0 = 周日，只标周一/三/五，其余行留空占位保持对齐。
const weekdayLabels = ["", "一", "", "三", "", "五", ""];

// 横轴月份标签：列首格月份相对上一列变化时，在该列左缘放一个标签。
const monthLabels = computed(() => {
  const total = cells.value.length;
  const cols = total / 7;
  const labels: { index: number; text: string; left: string }[] = [];
  let prevMonth = -1;
  for (let c = 0; c < cols; c++) {
    const date = cells.value[c * 7]?.date;
    if (!date) continue;
    const m = Number(date.slice(5, 7));
    if (m !== prevMonth) {
      labels.push({ index: c, text: `${m}月`, left: `${(c / cols) * 100}%` });
      prevMonth = m;
    }
  }
  return labels;
});

// --- F-19：悬浮提示（事件委托 + fixed 定位，跟随鼠标） ---
const tip = reactive({ show: false, text: "", x: 0, y: 0 });

function onCellHover(e: MouseEvent) {
  const el = (e.target as HTMLElement).closest<HTMLElement>(".heatmap-cell");
  const text = el?.dataset.tip;
  if (!text) {
    tip.show = false;
    return;
  }
  tip.show = true;
  tip.text = text;
  tip.x = e.clientX;
  tip.y = e.clientY;
}
</script>

<style scoped>
.commit-heatmap {
  /* F-21：纵轴宽度，月份行的左缩进与之对齐（标签对齐网格左缘） */
  --hm-y-axis-w: 22px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

/* F-21：月份行（横轴）。网格 grid-auto-columns: 1fr 永不横向溢出，
   月份标签按列百分比绝对定位即可与列对齐，无需跟随滚动。 */
.heatmap-months {
  position: relative;
  height: 14px;
  margin-left: calc(var(--hm-y-axis-w) + 3px);
  overflow: hidden;
}

.month-label {
  position: absolute;
  top: 0;
  font-size: 10px;
  line-height: 14px;
  color: var(--gw-text-dim);
  white-space: nowrap;
}

.heatmap-row {
  display: flex;
  gap: 3px;
}

/* F-21：纵轴与网格同高（flex stretch）+ 同 3px 间距，
   repeat(7, 1fr) 行高因此恒等于格子行高，星期标签逐行对齐。 */
.heatmap-y-axis {
  width: var(--hm-y-axis-w);
  flex-shrink: 0;
  display: grid;
  grid-template-rows: repeat(7, 1fr);
  gap: 3px;
}

.weekday-label {
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  line-height: 1;
  color: var(--gw-text-dim);
}

.heatmap-scroll {
  flex: 1;
  min-width: 0;
  overflow-x: auto;
  padding-bottom: 4px;
}

/* F-19：grid-auto-columns: 1fr 让所有隐式列平分容器宽度，网格横向占满 */
.heatmap-grid {
  display: grid;
  grid-template-rows: repeat(7, auto);
  grid-auto-columns: 1fr;
  grid-auto-flow: column;
  gap: 3px;
  width: 100%;
}

.heatmap-cell {
  width: 12px;
  height: 12px;
  border-radius: 2px;
  background: var(--gw-bg-hover);
}

/* F-19：仅网格内的格子随列宽拉伸（保持正方形）；图例方块保持 12px */
.heatmap-grid .heatmap-cell {
  width: 100%;
  height: auto;
  aspect-ratio: 1;
  min-width: 0;
}

.heatmap-grid .heatmap-cell[data-tip]:hover {
  outline: 1px solid var(--gw-accent);
}

.heatmap-tip {
  position: fixed;
  transform: translate(-50%, calc(-100% - 10px));
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-sm);
  padding: 4px 8px;
  font-size: var(--gw-text-xs);
  color: var(--gw-text);
  white-space: nowrap;
  pointer-events: none;
  z-index: 1000;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}

.level-0 {
  background: var(--gw-bg-hover);
}

.level-1 {
  background: color-mix(in srgb, var(--gw-success) 30%, transparent);
}

.level-2 {
  background: color-mix(in srgb, var(--gw-success) 55%, transparent);
}

.level-3 {
  background: color-mix(in srgb, var(--gw-success) 80%, transparent);
}

.level-4 {
  background: var(--gw-success);
}

.heatmap-legend {
  display: flex;
  align-items: center;
  gap: 3px;
  justify-content: flex-end;
}

.legend-text {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin: 0 4px;
}
</style>
