<template>
  <div class="commit-heatmap">
    <div class="heatmap-scroll">
      <div class="heatmap-grid">
        <div
          v-for="(cell, i) in cells"
          :key="i"
          class="heatmap-cell"
          :class="`level-${cell.level}`"
          :title="cell.date ? `${cell.date}：${cell.count} 次提交` : ''"
        ></div>
      </div>
    </div>
    <div class="heatmap-legend">
      <span class="legend-text">少</span>
      <span v-for="level in [0, 1, 2, 3, 4]" :key="level" class="heatmap-cell" :class="`level-${level}`"></span>
      <span class="legend-text">多</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";

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
</script>

<style scoped>
.commit-heatmap {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.heatmap-scroll {
  overflow-x: auto;
  padding-bottom: 4px;
}

.heatmap-grid {
  display: grid;
  grid-template-rows: repeat(7, 1fr);
  grid-auto-flow: column;
  gap: 3px;
  width: max-content;
}

.heatmap-cell {
  width: 12px;
  height: 12px;
  border-radius: 2px;
  background: #ebedf0;
}

.level-0 {
  background: #ebedf0;
}

.level-1 {
  background: #9be9a8;
}

.level-2 {
  background: #40c463;
}

.level-3 {
  background: #30a14e;
}

.level-4 {
  background: #216e39;
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
