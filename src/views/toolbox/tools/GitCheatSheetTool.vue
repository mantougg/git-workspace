<template>
  <div class="git-cheatsheet-tool">
    <div class="tool-actions">
      <n-input
        v-model:value="keyword"
        size="small"
        clearable
        placeholder="搜索场景或命令，如「强推」「rebase」…"
        class="search-input"
      >
        <template #prefix>
          <n-icon><SearchOutline /></n-icon>
        </template>
      </n-input>
      <span class="hint">点击命令即可复制</span>
    </div>

    <div v-for="group in filtered" :key="group.title" class="group">
      <div class="group-title">{{ group.title }}</div>
      <div v-for="(e, i) in group.entries" :key="i" class="entry">
        <div class="scenario">
          <span>{{ e.scenario }}</span>
          <span v-if="e.note" class="note">{{ e.note }}</span>
        </div>
        <button class="command mono" :title="'点击复制'" @click="copy(e.command)">
          {{ e.command }}
        </button>
      </div>
    </div>
    <n-empty v-if="filtered.length === 0" size="small" description="无匹配" />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { NEmpty, NIcon, NInput, useMessage } from "naive-ui";
import { SearchOutline } from "@vicons/ionicons5";
import { GIT_GROUPS } from "../data/gitCheatSheet";
import { errMsg } from "@/utils/error";

const message = useMessage();
const keyword = ref("");

const filtered = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return GIT_GROUPS;
  return GIT_GROUPS.map((g) => ({
    ...g,
    entries: g.entries.filter(
      (e) =>
        e.scenario.toLowerCase().includes(kw) ||
        e.command.toLowerCase().includes(kw) ||
        (e.note ?? "").toLowerCase().includes(kw),
    ),
  })).filter((g) => g.entries.length > 0);
});

async function copy(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success("已复制");
  } catch (e) {
    message.error("复制失败：" + errMsg(e));
  }
}
</script>

<style scoped>
.git-cheatsheet-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.tool-actions {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}

.search-input {
  width: 280px;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.group + .group {
  margin-top: var(--gw-space-3);
}

.group-title {
  font-size: var(--gw-text-sm);
  font-weight: 600;
  color: var(--gw-text-dim);
  padding: var(--gw-space-1) 0;
}

.entry {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-3);
  padding: var(--gw-space-1) 0;
}

.scenario {
  display: flex;
  flex-direction: column;
  font-size: var(--gw-text-sm);
}

.note {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-xs);
}

.command {
  flex-shrink: 0;
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  background: var(--gw-bg-hover);
  color: var(--gw-text);
  padding: var(--gw-space-1) var(--gw-space-2);
  cursor: copy;
  transition: border-color 0.15s;
}

.command:hover {
  border-color: var(--gw-accent);
  color: var(--gw-accent);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
</style>
