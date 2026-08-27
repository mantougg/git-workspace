<template>
  <n-modal
    :show="show"
    :auto-focus="false"
    :mask-closable="true"
    transform-origin="center"
    @update:show="onUpdateShow"
  >
    <div class="command-palette">
      <!-- 搜索输入 -->
      <div class="command-palette-input">
        <n-input
          ref="inputRef"
          v-model:value="query"
          placeholder="输入命令或搜索视图…"
          clearable
          @keydown="onKeydown"
        >
          <template #prefix>
            <n-icon :size="16"><SearchOutline /></n-icon>
          </template>
        </n-input>
      </div>

      <!-- 结果列表 -->
      <div class="command-palette-results">
        <div v-if="groupedResults.length === 0" class="command-palette-empty">
          无匹配命令
        </div>
        <template v-for="group in groupedResults" :key="group.label">
          <div class="command-palette-group-label">{{ group.label }}</div>
          <div
            v-for="(cmd, idx) in group.items"
            :key="cmd.id"
            class="command-palette-item"
            :class="{ active: isActiveIndex(group.offset + idx) }"
            @click="execute(cmd)"
            @mouseenter="activeIndex = group.offset + idx"
          >
            <span class="command-palette-item-title">{{ cmd.title }}</span>
            <span v-if="cmd.shortcut" class="command-palette-item-shortcut">{{ cmd.shortcut }}</span>
          </div>
        </template>
      </div>
    </div>
  </n-modal>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick } from "vue";
import { NModal, NInput, NIcon } from "naive-ui";
import { SearchOutline } from "@vicons/ionicons5";
import { getAllCommands, type Command } from "@/commands/registry";

const props = defineProps<{
  show: boolean;
}>();

const emit = defineEmits<{
  (e: "update:show", value: boolean): void;
}>();

const query = ref("");
const activeIndex = ref(0);
const inputRef = ref<InstanceType<typeof NInput> | null>(null);

const allCommands = computed(() => getAllCommands());

/** 模糊搜索过滤 */
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return allCommands.value;
  return allCommands.value.filter(
    (cmd) =>
      cmd.title.toLowerCase().includes(q) ||
      cmd.group.toLowerCase().includes(q)
  );
});

/** 按 group 分组 */
const groupedResults = computed(() => {
  const groups = new Map<string, { items: Command[]; offset: number }>();
  let offset = 0;
  for (const cmd of filtered.value) {
    if (!groups.has(cmd.group)) {
      groups.set(cmd.group, { items: [], offset });
    }
    groups.get(cmd.group)!.items.push(cmd);
    offset++;
  }
  return Array.from(groups.entries()).map(([label, data]) => ({
    label,
    ...data,
  }));
});

const totalItems = computed(() => filtered.value.length);

function isActiveIndex(idx: number): boolean {
  return activeIndex.value === idx;
}

function execute(cmd: Command) {
  cmd.run();
  close();
}

function close() {
  emit("update:show", false);
  query.value = "";
  activeIndex.value = 0;
}

function onUpdateShow(val: boolean) {
  if (!val) close();
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === "ArrowDown") {
    e.preventDefault();
    activeIndex.value = (activeIndex.value + 1) % Math.max(totalItems.value, 1);
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    activeIndex.value =
      (activeIndex.value - 1 + Math.max(totalItems.value, 1)) %
      Math.max(totalItems.value, 1);
  } else if (e.key === "Enter") {
    e.preventDefault();
    const cmd = filtered.value[activeIndex.value];
    if (cmd) execute(cmd);
  } else if (e.key === "Escape") {
    e.preventDefault();
    close();
  }
}

// 打开时自动聚焦
watch(
  () => props.show,
  async (val) => {
    if (val) {
      activeIndex.value = 0;
      await nextTick();
      inputRef.value?.focus();
    }
  }
);
</script>

<style scoped>
.command-palette {
  width: 520px;
  max-height: 420px;
  background: var(--gw-bg-panel);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.command-palette-input {
  padding: var(--gw-space-2);
  border-bottom: 1px solid var(--gw-border);
}

.command-palette-results {
  flex: 1;
  overflow-y: auto;
  padding: var(--gw-space-1) 0;
}

.command-palette-empty {
  padding: var(--gw-space-4);
  text-align: center;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.command-palette-group-label {
  padding: var(--gw-space-1) var(--gw-space-3);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  font-weight: 500;
}

.command-palette-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--gw-space-2) var(--gw-space-3);
  cursor: pointer;
  font-size: var(--gw-text-md);
  color: var(--gw-text);
  transition: background 0.1s;
}

.command-palette-item:hover,
.command-palette-item.active {
  background: var(--gw-bg-hover);
}

.command-palette-item-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.command-palette-item-shortcut {
  margin-left: var(--gw-space-2);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  flex-shrink: 0;
}
</style>
