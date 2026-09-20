<template>
  <!-- 「查看整个文件」模式：虚拟滚动渲染全文，行号栏叠加 git 变更状态色条
       （IDEA 风格：绿=新增、蓝=修改、红标记=删除），与 UnifiedDiff 同源数据。 -->
  <VirtualList v-if="rows.length > 0" :items="rows" :item-height="ROW_HEIGHT" class="file-content">
    <template #row="{ item }">
      <div v-if="item.tail" class="file-line tail">
        <span class="gutter"><span class="status-bar deleted" /></span>
        <span class="line-num" />
        <span class="line-content tail-text">{{ item.text }}</span>
      </div>
      <div v-else class="file-line">
        <span class="gutter">
          <span
            v-if="item.deletedBefore > 0"
            class="del-marker"
            :title="`${item.deletedBefore} 行已删除`"
          />
          <span v-if="item.status" class="status-bar" :class="item.status" />
        </span>
        <span class="line-num">{{ item.lineNo }}</span>
        <span class="line-content">{{ item.content }}</span>
      </div>
    </template>
  </VirtualList>
  <div v-else class="file-empty">空文件</div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import VirtualList from "@/components/common/VirtualList.vue";
import type { FileLineStatus } from "@/utils/diffStatus";

/** Fixed row height (px) required by VirtualList — 与 UnifiedDiff 对齐。 */
const ROW_HEIGHT = 21;

const props = defineProps<{
  /** 文件全文（后端 read_workdir_file）。 */
  lines: string[];
  /** 由 diff hunks 投影出的行状态（buildFileLineStatus）。 */
  lineStatus: FileLineStatus;
}>();

interface Row {
  lineNo: number | null;
  content: string;
  status?: "added" | "modified";
  deletedBefore: number;
  tail?: boolean;
  text?: string;
}

const rows = computed<Row[]>(() => {
  const out: Row[] = props.lines.map((content, i) => {
    const lineNo = i + 1;
    return {
      lineNo,
      content,
      status: props.lineStatus.status.get(lineNo),
      deletedBefore: props.lineStatus.deletedBefore.get(lineNo) ?? 0,
    };
  });
  if (props.lineStatus.deletedAtEnd > 0) {
    out.push({
      lineNo: null,
      content: "",
      deletedBefore: 0,
      tail: true,
      text: `… ${props.lineStatus.deletedAtEnd} 行已在文件末尾删除`,
    });
  }
  return out;
});
</script>

<style scoped>
.file-content {
  font-family: var(--gw-font-mono);
  font-size: 13px;
}

.file-line {
  display: flex;
  align-items: center;
  height: 21px;
  line-height: 21px;
  white-space: pre;
}

.gutter {
  position: relative;
  align-self: stretch;
  width: 6px;
  flex-shrink: 0;
}

/* 行号栏状态色条：绿=新增、蓝=修改（与 diff 行背景色语义一致）。 */
.status-bar {
  position: absolute;
  left: 1px;
  top: 0;
  bottom: 0;
  width: 3px;
}

.status-bar.added {
  background: var(--gw-success);
}

.status-bar.modified {
  background: var(--gw-accent);
}

.status-bar.deleted {
  background: var(--gw-danger);
}

/* 删除标记：贴在该行顶部的红色小角标（IDEA gutter 同款语义）。 */
.del-marker {
  position: absolute;
  left: 1px;
  top: 0;
  width: 0;
  height: 0;
  border-left: 3px solid transparent;
  border-right: 3px solid transparent;
  border-top: 5px solid var(--gw-danger);
}

.line-num {
  display: inline-block;
  width: 44px;
  text-align: right;
  padding-right: 8px;
  color: var(--gw-text-dim);
  user-select: none;
  flex-shrink: 0;
}

.line-content {
  flex: 1;
  padding-right: 8px;
}

.file-empty {
  padding: var(--gw-space-4);
  color: var(--gw-text-dim);
  font-size: 13px;
}

.tail-text {
  color: var(--gw-text-dim);
  font-size: 12px;
}
</style>
