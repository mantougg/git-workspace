<template>
  <div class="symbol-view">
    <!-- Header -->
    <div class="symbol-header">
      <RepoSwitcher @change="onRepoSwitch" />
      <n-button size="small" :loading="indexing" @click="buildIndex">
        <template #icon><n-icon><HammerOutline /></n-icon></template>
        {{ indexed ? "更新索引" : "构建索引" }}
      </n-button>
      <n-input
        v-model:value="query"
        size="small"
        class="symbol-query"
        placeholder="符号名…（过滤器：@ext:rs,ts @path:src @repo:x @group:y @status:dirty）"
        clearable
        @keydown.enter="search"
      />
      <n-button size="small" type="primary" :loading="searching" @click="search">
        <template #icon><n-icon><SearchOutline /></n-icon></template>
        搜索
      </n-button>
    </div>

    <div class="symbol-body">
      <!-- 结果列表 -->
      <div class="symbol-results">
        <div
          v-for="hit in results"
          :key="hit.repoPath + hit.filePath + hit.name + hit.line"
          class="symbol-row"
          :class="{ active: selected?.name === hit.name }"
          @click="select(hit)"
        >
          <n-tag size="small" :bordered="false">{{ hit.kind }}</n-tag>
          <span class="sym-name">{{ hit.name }}</span>
          <span v-if="hit.container" class="sym-container">in {{ hit.container }}</span>
          <span class="sym-file">{{ shortFile(hit) }}:{{ hit.line }}</span>
        </div>
        <n-empty
          v-if="!searching && results.length === 0"
          description="输入符号名搜索（先构建索引）"
          class="symbol-empty"
        />
      </div>

      <!-- 详情：定义 / 引用 / 调用层级 -->
      <div v-if="selected" class="symbol-detail">
        <div class="detail-title">
          <n-tag size="small" :bordered="false">{{ selected.kind }}</n-tag>
          <span class="detail-name">{{ selected.name }}</span>
          <span v-if="selected.signature" class="detail-signature">{{ selected.signature }}</span>
        </div>

        <div class="detail-section">
          <div class="section-title">
            定义
            <n-button quaternary size="tiny" @click="expand('definitions')">查看</n-button>
          </div>
          <div v-for="d in detail.definitions" :key="d.repoPath + d.filePath + d.line" class="ref-row">
            <span class="ref-file">{{ shortFile(d) }}:{{ d.line }}</span>
            <span class="ref-repo">{{ repoName(d.repoPath) }}</span>
          </div>
        </div>

        <div class="detail-section">
          <div class="section-title">
            引用
            <n-button quaternary size="tiny" @click="expand('references')">查看</n-button>
          </div>
          <div v-for="r in detail.references" :key="r.repoPath + r.filePath + r.line" class="ref-row">
            <span class="ref-file">{{ r.filePath }}:{{ r.line }}</span>
            <n-tag v-if="r.isCall" size="tiny" type="info" :bordered="false">调用</n-tag>
            <span class="ref-repo">{{ repoName(r.repoPath) }}</span>
          </div>
        </div>

        <div class="detail-section">
          <div class="section-title">
            调用层级
            <n-button quaternary size="tiny" :loading="detail.callersLoading" @click="expand('callers')">
              谁调用
            </n-button>
            <n-button quaternary size="tiny" :loading="detail.calleesLoading" @click="expand('callees')">
              调用了谁
            </n-button>
          </div>
          <div v-for="c in detail.callers" :key="'caller' + c.repoPath + c.filePath + c.line" class="ref-row">
            <span class="ref-file">{{ c.name }}（{{ c.kind }}，×{{ c.callCount }}）</span>
            <span class="ref-repo">{{ shortPath(c.filePath) }}:{{ c.line }}</span>
          </div>
          <div v-for="c in detail.callees" :key="'callee' + c.repoPath + c.filePath + c.line" class="ref-row">
            <span class="ref-file">{{ c.name }}（{{ c.kind }}，×{{ c.callCount }}）</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useMessage } from "naive-ui";
import { HammerOutline, SearchOutline } from "@vicons/ionicons5";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import { useWorkspaceStore } from "@/stores/workspace";
import {
  buildSymbolIndex,
  findSymbolDefinitions,
  findSymbolReferences,
  searchSymbols,
  symbolCallHierarchy,
} from "@/api/symbols";
import type {
  SymbolCallHit,
  SymbolHit,
  SymbolIndexStats,
  SymbolRefHit,
} from "@/types/symbols";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { resolveCurrentRepo } = useCurrentRepo();
const workspaceStore = useWorkspaceStore();

const repoPath = ref("");
const indexed = ref(false);
const indexing = ref(false);
const searching = ref(false);
const query = ref("");
const results = ref<SymbolHit[]>([]);
const selected = ref<SymbolHit | null>(null);

const detail = reactive<{
  definitions: SymbolHit[];
  references: SymbolRefHit[];
  callers: SymbolCallHit[];
  callees: SymbolCallHit[];
  callersLoading: boolean;
  calleesLoading: boolean;
}>({
  definitions: [],
  references: [],
  callers: [],
  callees: [],
  callersLoading: false,
  calleesLoading: false,
});

onMounted(async () => {
  // F-14/F-17：query → 全局当前仓库 → 工作区首仓库兜底（SideNav 直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
});

async function onRepoSwitch(path: string) {
  repoPath.value = path;
  indexed.value = false;
  results.value = [];
  selected.value = null;
  clearDetail();
}

async function buildIndex() {
  if (!repoPath.value) return;
  indexing.value = true;
  try {
    const stats: SymbolIndexStats = await buildSymbolIndex(repoPath.value);
    indexed.value = true;
    message.success(
      `索引完成：扫描 ${stats.filesScanned} 文件，重建 ${stats.filesReindexed}，` +
        `跳过 ${stats.filesSkipped}，符号 ${stats.symbols}，引用 ${stats.refs}`,
    );
  } catch (e) {
    message.error("构建符号索引失败: " + errMsg(e));
  } finally {
    indexing.value = false;
  }
}

async function search() {
  const q = query.value.trim();
  if (!q) return;
  searching.value = true;
  selected.value = null;
  clearDetail();
  try {
    results.value = await searchSymbols(
      q,
      workspaceStore.currentWorkspace?.id,
    );
  } catch (e) {
    results.value = [];
    message.error("符号搜索失败: " + errMsg(e));
  } finally {
    searching.value = false;
  }
}

function select(hit: SymbolHit) {
  selected.value = hit;
  clearDetail();
  // 选中即拉定义与引用（同一符号名），调用层级按需展开。
  void expand("definitions");
  void expand("references");
}

function clearDetail() {
  detail.definitions = [];
  detail.references = [];
  detail.callers = [];
  detail.callees = [];
}

async function expand(section: "definitions" | "references" | "callers" | "callees") {
  if (!selected.value) return;
  const name = selected.value.name;
  const wsId = workspaceStore.currentWorkspace?.id;
  try {
    switch (section) {
      case "definitions":
        detail.definitions = await findSymbolDefinitions(name, undefined, wsId);
        break;
      case "references":
        detail.references = await findSymbolReferences(name, undefined, wsId);
        break;
      case "callers": {
        detail.callersLoading = true;
        try {
          detail.callers = await symbolCallHierarchy(name, "callers", undefined, wsId);
        } finally {
          detail.callersLoading = false;
        }
        break;
      }
      case "callees": {
        detail.calleesLoading = true;
        try {
          detail.callees = await symbolCallHierarchy(name, "callees", undefined, wsId);
        } finally {
          detail.calleesLoading = false;
        }
        break;
      }
    }
  } catch (e) {
    message.error("查询失败: " + errMsg(e));
  }
}

function shortFile(hit: SymbolHit): string {
  return shortPath(hit.filePath);
}

function shortPath(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return parts.slice(-2).join("/");
}

function repoName(path: string): string {
  return path.replace(/\\/g, "/").split("/").filter(Boolean).pop() ?? path;
}
</script>

<style scoped>
.symbol-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.symbol-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.symbol-query {
  flex: 1;
  min-width: 0;
}

.symbol-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  overflow-y: auto;
  background: var(--gw-bg-panel);
}

.symbol-results {
  display: flex;
  flex-direction: column;
}

.symbol-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 16px;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
  cursor: pointer;
}

.symbol-row:hover,
.symbol-row.active {
  background: var(--gw-bg-hover);
}

.sym-name {
  font-family: var(--gw-font-mono);
  color: var(--gw-accent);
  flex-shrink: 0;
}

.sym-container {
  color: var(--gw-text-dim);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sym-file {
  margin-left: auto;
  flex-shrink: 0;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
}

.symbol-empty {
  margin-top: var(--gw-space-8);
}

.symbol-detail {
  border-top: 1px solid var(--gw-border);
  padding: var(--gw-space-3) 16px;
  background: var(--gw-bg-panel);
}

.detail-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  margin-bottom: var(--gw-space-2);
}

.detail-name {
  font-family: var(--gw-font-mono);
  font-weight: 600;
}

.detail-signature {
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-section {
  margin-bottom: var(--gw-space-3);
}

.section-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  font-size: 12px;
  color: var(--gw-text-dim);
  margin-bottom: 4px;
}

.ref-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 2px 0;
  font-size: 12px;
  font-family: var(--gw-font-mono);
}

.ref-file {
  color: var(--gw-text);
}

.ref-repo {
  color: var(--gw-text-dim);
  margin-left: auto;
}
</style>
