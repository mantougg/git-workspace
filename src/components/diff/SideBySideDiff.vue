<template>
  <!-- F-56：零 hunk（纯行尾符/权限差异、二进制文件等）时给出明确空态，
       避免空白面板被误认为功能故障。 -->
  <n-empty
    v-if="rows.length === 0"
    class="diff-empty"
    description="没有可展示的行级差异（可能仅行尾符/权限变化，或为二进制文件）"
  />
  <VirtualList v-else :items="rows" :item-height="ROW_HEIGHT" class="side-by-side-diff">
    <template #row="{ item }">
      <div v-if="item.type === 'header'" class="hunk-header">
        {{ item.text }}
      </div>
      <div v-else class="diff-row">
        <div
          :class="['diff-cell', item.old?.type]"
          :title="item.old?.content"
        >
          <template v-if="item.old">
            <span class="gutter-bar" :class="item.old.status" />
            <span class="line-num">{{ item.old.num ?? "" }}</span>
            <span class="line-prefix">{{ item.old.type === "delete" ? "-" : " " }}</span>
            <span class="line-content">{{ item.old.content }}</span>
          </template>
        </div>
        <div
          :class="['diff-cell', item.new?.type]"
          :title="item.new?.content"
        >
          <template v-if="item.new">
            <span class="gutter-bar" :class="item.new.status" />
            <span class="line-num">{{ item.new.num ?? "" }}</span>
            <span class="line-prefix">{{ item.new.type === "add" ? "+" : " " }}</span>
            <span class="line-content">{{ item.new.content }}</span>
          </template>
        </div>
      </div>
    </template>
  </VirtualList>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { NEmpty } from "naive-ui";
import VirtualList from "@/components/common/VirtualList.vue";
import type { FileDiff } from "@/types/git";
import { refineHunkLines, type RefinedLineStatus } from "@/utils/diffStatus";

const props = defineProps<{
  file: FileDiff;
}>();

/** Fixed row height (px) required by VirtualList. */
const ROW_HEIGHT = 21;

interface AlignedLine {
  num: number | null;
  content: string;
  type: string;
  /** IDEA 风格精化状态，驱动行号栏色条（与 UnifiedDiff 同源）。 */
  status: RefinedLineStatus;
}

type Row =
  | { type: "header"; text: string }
  | { type: "pair"; old: AlignedLine | null; new: AlignedLine | null };

// Flatten hunks into header + paired old/new rows so a single virtual window
// bounds the DOM node count (T-04 frontend rendering budget).
//
// 配对规则：一段连续的 `-` 后紧跟一段连续的 `+` 视为同一个「修改块」，
// 两侧按 1:1 对齐（多出的一侧单独成行）——按索引硬配会在两侧行数
// 不等时把不相关的行对到一起。context 行两侧同时出现。
const rows = computed<Row[]>(() => {
  const out: Row[] = [];
  for (const hunk of props.file.hunks) {
    out.push({
      type: "header",
      text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    });
    const refined = refineHunkLines(hunk);
    let dels: AlignedLine[] = [];
    let adds: AlignedLine[] = [];
    const flush = () => {
      const n = Math.max(dels.length, adds.length);
      for (let i = 0; i < n; i++) {
        out.push({ type: "pair", old: dels[i] ?? null, new: adds[i] ?? null });
      }
      dels = [];
      adds = [];
    };
    hunk.lines.forEach((l, i) => {
      const status = refined[i];
      if (l.lineType === "delete") {
        dels.push({ num: l.oldLine, content: l.content, type: l.lineType, status });
      } else if (l.lineType === "add") {
        adds.push({ num: l.newLine, content: l.content, type: l.lineType, status });
      } else {
        flush();
        out.push({
          type: "pair",
          old: { num: l.oldLine, content: l.content, type: "context", status },
          new: { num: l.newLine, content: l.content, type: "context", status },
        });
      }
    });
    flush();
  }
  return out;
});
</script>

<style scoped>
.diff-empty {
  margin-top: var(--gw-space-4);
}

.side-by-side-diff {
  font-family: var(--gw-font-mono);
  font-size: 13px;
}

.hunk-header {
  height: 21px;
  line-height: 21px;
  background: var(--gw-border);
  color: var(--gw-text-dim);
  padding: 0 8px;
  font-size: 12px;
  white-space: pre;
}

.diff-row {
  display: flex;
  height: 21px;
}

/* Virtual rows are fixed-height, so long lines cannot wrap (that would break
   the height math); they clip with an ellipsis and stay readable via the
   title tooltip. */
.diff-cell {
  display: flex;
  align-items: center;
  flex: 1;
  min-width: 0;
  height: 21px;
  line-height: 21px;
  padding: 0 8px;
  white-space: pre;
  overflow: hidden;
}

.diff-cell:first-child {
  border-right: 1px solid var(--gw-border);
}

.diff-cell.delete {
  background: color-mix(in srgb, var(--gw-danger) 12%, transparent);
}

.diff-cell.add {
  background: color-mix(in srgb, var(--gw-success) 12%, transparent);
}

.diff-cell.context {
  background: var(--gw-bg-hover);
}

/* 行号栏 git 状态色条（IDEA 风格）：绿=新增、蓝=修改、红=删除。 */
.gutter-bar {
  align-self: stretch;
  width: 3px;
  flex-shrink: 0;
  margin-right: 5px;
}

.gutter-bar.added {
  background: var(--gw-success);
}

.gutter-bar.modified {
  background: var(--gw-accent);
}

.gutter-bar.deleted {
  background: var(--gw-danger);
}

.gutter-bar.context {
  background: transparent;
}

.line-num {
  display: inline-block;
  width: 40px;
  text-align: right;
  color: var(--gw-text-dim);
  user-select: none;
  flex-shrink: 0;
}

.line-prefix {
  display: inline-block;
  width: 16px;
  text-align: center;
  flex-shrink: 0;
}

.line-content {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
