<template>
  <n-modal
    :show="visible"
    preset="card"
    title="Rebase"
    style="width: 760px"
    :close-on-click-modal="false"
    @update:show="(v: boolean) => { if (!v) visible = false }"
  >
    <div class="rebase-form">
      <span class="form-label">Onto（目标基点）：</span>
      <n-select v-model:value="onto" filterable style="width: 240px" :options="revisionOptions" @update:value="loadOps" />
      <n-radio-group v-model:value="mode" @update:value="loadOps">
        <n-radio value="normal">普通</n-radio>
        <n-radio value="interactive">Interactive</n-radio>
      </n-radio-group>
    </div>

    <div v-if="mode === 'interactive'" class="op-editor">
      <n-spin :show="opsLoading">
        <div class="op-hint">
          拖拽或 ↑↓ 调整顺序；pick 应用，reword 改消息，squash 并入上一条，drop 跳过。
        </div>
        <div
          v-for="(op, i) in ops"
          :key="op.oid"
          :class="['op-row', { dropped: op.action === 'drop' }]"
          draggable="true"
          @dragstart="dragIndex = i"
          @dragover.prevent
          @drop="onDrop(i)"
        >
          <n-select
            v-model:value="op.action"
            size="small"
            style="width: 104px"
            :options="actionOptions"
            @update:value="(v: string) => onActionChange(op, v)"
          />
          <span class="op-oid">{{ op.oid.slice(0, 7) }}</span>
          <n-input
            v-if="op.action === 'reword'"
            v-model:value="op.message"
            size="small"
            class="op-message"
            :placeholder="op.subject"
          />
          <span v-else class="op-subject">{{ op.subject }}</span>
          <n-button size="small" text :disabled="i === 0" @click="move(i, -1)">↑</n-button>
          <n-button size="small" text :disabled="i === ops.length - 1" @click="move(i, 1)">↓</n-button>
        </div>
        <n-empty
          v-if="!opsLoading && ops.length === 0"
          description="该基点之上没有需要 rebase 的提交"
        />
      </n-spin>
    </div>

    <template #footer>
      <n-button @click="visible = false">取消</n-button>
      <n-button type="primary" :loading="running" @click="run">开始 Rebase</n-button>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { useMessage } from "naive-ui";
import { listRebaseCommits, startRebase } from "@/api/rebase";
import type { RebaseOp, RebaseOutcome } from "@/types/rebase";
import { errMsg } from "@/utils/error";

const message = useMessage();

const props = defineProps<{
  repoPath: string;
  /** Candidate onto revisions (local + remote branch names). */
  revisions: string[];
  /** Preselected onto target. */
  defaultOnto: string;
}>();

const emit = defineEmits<{
  /** Rebase finished (success) or interrupted (conflict); null = cancelled/failed. */
  (e: "finished", outcome: RebaseOutcome | null): void;
}>();

const visible = defineModel<boolean>({ required: true });

const onto = ref("");
const mode = ref<"normal" | "interactive">("normal");
const ops = ref<RebaseOp[]>([]);
const opsLoading = ref(false);
const running = ref(false);
const dragIndex = ref<number | null>(null);

const revisionOptions = computed(() =>
  props.revisions.map((r) => ({ label: r, value: r })),
);

const actionOptions = [
  { label: "pick", value: "pick" },
  { label: "reword", value: "reword" },
  { label: "squash", value: "squash" },
  { label: "drop", value: "drop" },
];

// Reload the default todo whenever the dialog opens or onto/mode changes.
watch(visible, (v) => {
  if (v) {
    onto.value = props.defaultOnto;
    mode.value = "normal";
    ops.value = [];
  }
});

async function loadOps() {
  if (mode.value !== "interactive" || !onto.value) return;
  opsLoading.value = true;
  try {
    ops.value = await listRebaseCommits(props.repoPath, onto.value);
  } catch (e) {
    message.error("加载 rebase 提交列表失败: " + errMsg(e));
  } finally {
    opsLoading.value = false;
  }
}

function onActionChange(op: RebaseOp, action: string) {
  if (action === "reword" && !op.message) {
    op.message = op.subject;
  }
}

function move(i: number, delta: number) {
  const j = i + delta;
  if (j < 0 || j >= ops.value.length) return;
  const arr = [...ops.value];
  [arr[i], arr[j]] = [arr[j], arr[i]];
  ops.value = arr;
}

function onDrop(target: number) {
  if (dragIndex.value === null || dragIndex.value === target) return;
  const arr = [...ops.value];
  const [item] = arr.splice(dragIndex.value, 1);
  arr.splice(target, 0, item);
  ops.value = arr;
  dragIndex.value = null;
}

async function run() {
  if (!onto.value) {
    message.warning("请选择目标基点（onto）");
    return;
  }
  running.value = true;
  try {
    // Normal mode: backend-default pick todo; interactive: the arranged ops.
    const todo =
      mode.value === "interactive"
        ? ops.value
        : await listRebaseCommits(props.repoPath, onto.value);
    const outcome = await startRebase(props.repoPath, onto.value, todo);
    visible.value = false;
    emit("finished", outcome);
  } catch (e) {
    message.error("Rebase 失败: " + errMsg(e));
    emit("finished", null);
  } finally {
    running.value = false;
  }
}
</script>

<style scoped>
.rebase-form {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.form-label {
  font-size: 13px;
  color: #606266;
}

.op-editor {
  border: 1px solid #ebeef5;
  border-radius: 4px;
  max-height: 50vh;
  overflow-y: auto;
  padding: 8px;
}

.op-hint {
  font-size: 12px;
  color: #909399;
  margin-bottom: 8px;
}

.op-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px;
  border-bottom: 1px solid #f5f5f5;
  cursor: grab;
  background: #fff;
}

.op-row.dropped .op-subject {
  text-decoration: line-through;
  color: #c0c4cc;
}

.op-oid {
  font-family: "Cascadia Code", Consolas, monospace;
  color: #409eff;
  font-size: 12px;
  flex-shrink: 0;
}

.op-subject {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}

.op-message {
  flex: 1;
}
</style>
