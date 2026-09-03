<template>
  <div class="http-status-tool">
    <div class="tool-actions">
      <n-input
        v-model:value="keyword"
        size="small"
        clearable
        placeholder="搜索状态码 / 名称 / 关键词…"
        class="search-input"
      >
        <template #prefix>
          <n-icon><SearchOutline /></n-icon>
        </template>
      </n-input>
    </div>

    <n-tabs v-model:value="tab" type="line" size="small">
      <n-tab-pane name="status" tab="状态码">
        <div v-for="group in filteredStatus" :key="group.cls" class="group">
          <div class="group-title">{{ group.cls }} {{ group.title }}</div>
          <div v-for="e in group.entries" :key="e.code" class="entry">
            <span class="mono code">{{ e.code }}</span>
            <span class="name">{{ e.name }}</span>
            <span class="desc">{{ e.desc }}</span>
          </div>
        </div>
        <n-empty v-if="filteredStatus.length === 0" size="small" description="无匹配" />
      </n-tab-pane>

      <n-tab-pane name="req" tab="常用请求头">
        <div class="group">
          <div v-for="e in filteredReq" :key="e.name" class="entry">
            <span class="mono header-name">{{ e.name }}</span>
            <span class="desc">{{ e.desc }}</span>
          </div>
        </div>
        <n-empty v-if="filteredReq.length === 0" size="small" description="无匹配" />
      </n-tab-pane>

      <n-tab-pane name="resp" tab="常用响应头">
        <div class="group">
          <div v-for="e in filteredResp" :key="e.name" class="entry">
            <span class="mono header-name">{{ e.name }}</span>
            <span class="desc">{{ e.desc }}</span>
          </div>
        </div>
        <n-empty v-if="filteredResp.length === 0" size="small" description="无匹配" />
      </n-tab-pane>
    </n-tabs>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { NEmpty, NIcon, NInput, NTabPane, NTabs } from "naive-ui";
import { SearchOutline } from "@vicons/ionicons5";
import {
  REQUEST_HEADERS,
  RESPONSE_HEADERS,
  STATUS_GROUPS,
  type HeaderEntry,
} from "../data/httpStatus";

const keyword = ref("");
const tab = ref("status");

function matchHeader(e: HeaderEntry, kw: string): boolean {
  return e.name.toLowerCase().includes(kw) || e.desc.toLowerCase().includes(kw);
}

const filteredStatus = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return STATUS_GROUPS;
  return STATUS_GROUPS.map((g) => ({
    ...g,
    entries: g.entries.filter(
      (e) =>
        String(e.code).includes(kw) ||
        e.name.toLowerCase().includes(kw) ||
        e.desc.toLowerCase().includes(kw),
    ),
  })).filter((g) => g.entries.length > 0);
});

const filteredReq = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return REQUEST_HEADERS;
  return REQUEST_HEADERS.filter((e) => matchHeader(e, kw));
});

const filteredResp = computed(() => {
  const kw = keyword.value.trim().toLowerCase();
  if (!kw) return RESPONSE_HEADERS;
  return RESPONSE_HEADERS.filter((e) => matchHeader(e, kw));
});
</script>

<style scoped>
.http-status-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.search-input {
  width: 280px;
}

.group {
  display: flex;
  flex-direction: column;
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
  align-items: baseline;
  gap: var(--gw-space-2);
  padding: var(--gw-space-1) 0;
}

.code {
  width: 36px;
  flex-shrink: 0;
  color: var(--gw-accent);
  font-weight: 600;
}

.name {
  width: 220px;
  flex-shrink: 0;
  font-size: var(--gw-text-sm);
}

.header-name {
  width: 240px;
  flex-shrink: 0;
  color: var(--gw-accent);
}

.desc {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
</style>
