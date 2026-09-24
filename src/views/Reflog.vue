<template>
  <div class="reflog-view">
    <!-- Header -->
    <div class="reflog-header">
      <RepoSwitcher @change="onRepoSwitch" />
      <n-select
        v-model:value="reference"
        :options="referenceOptions"
        style="width: 220px"
        size="small"
        @update:value="load"
      />
      <n-button size="small" :loading="loading" @click="load">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
    </div>

    <!-- Entry list -->
    <n-spin :show="loading">
      <div class="reflog-body">
        <div v-for="entry in entries" :key="entry.selector" class="reflog-row">
          <span class="selector">{{ entry.selector }}</span>
          <span class="summary" :title="entry.summary">{{ entry.summary }}</span>
          <span class="commit-message" :title="entry.newOid">
            {{ entry.newOid.slice(0, 7) }} {{ entry.commitMessage }}
          </span>
          <span class="time">{{ entry.time }}</span>
          <n-dropdown trigger="click" :options="dropdownOptions" @select="(cmd: string) => onAction(cmd, entry)">
            <n-button size="small" text @click.stop>
              <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
            </n-button>
          </n-dropdown>
        </div>
        <n-empty v-if="!loading && entries.length === 0" description="暂无 reflog 记录" />
      </div>
    </n-spin>

    <!-- GF-13a：条数提示 + 加载更多（后端 max 已参数化，前端在此补翻页状态）。
         后端暂不返回总数：按页大小探测，短页即到底。 -->
    <div v-if="entries.length > 0 && (hasMore || allLoaded)" class="reflog-footer">
      <span class="reflog-count">
        {{ allLoaded ? `已显示全部 ${entries.length} 条` : `已显示前 ${entries.length} 条` }}
      </span>
      <n-button v-if="hasMore" size="small" :loading="loadingMore" @click="loadMore">
        加载更多
      </n-button>
    </div>

    <!-- View Commit dialog -->
    <n-modal v-model:show="viewDialog.show" preset="card" title="提交详情" style="width: 520px">
      <n-descriptions v-if="viewDialog.entry" :column="1" bordered label-placement="left">
        <n-descriptions-item label="位置">{{ viewDialog.entry.selector }}</n-descriptions-item>
        <n-descriptions-item label="Hash">{{ viewDialog.entry.newOid }}</n-descriptions-item>
        <n-descriptions-item label="提交信息">{{ viewDialog.entry.commitMessage }}</n-descriptions-item>
        <n-descriptions-item label="时间">{{ viewDialog.entry.time }}</n-descriptions-item>
        <n-descriptions-item label="Reflog 动作">{{ viewDialog.entry.summary }}</n-descriptions-item>
      </n-descriptions>
    </n-modal>

    <!-- Reset Here dialog -->
    <n-modal v-model:show="resetDialog.show" preset="card" title="Reset Here" style="width: 520px">
      <div v-if="resetDialog.entry" class="reset-target">
        目标：{{ resetDialog.entry.newOid.slice(0, 7) }} {{ resetDialog.entry.commitMessage }}
        （{{ resetDialog.entry.selector }}）
      </div>
      <n-radio-group v-model:value="resetDialog.mode" class="reset-modes">
        <n-radio value="soft">soft — 仅移动 HEAD，保留暂存区与工作区</n-radio>
        <n-radio value="mixed">mixed — 移动 HEAD + 重置暂存区，保留工作区</n-radio>
        <n-radio value="hard">hard — 重置全部，丢弃未提交更改（危险）</n-radio>
      </n-radio-group>
      <template #footer>
        <n-button @click="resetDialog.show = false">取消</n-button>
        <n-button
          :type="resetDialog.mode === 'hard' ? 'error' : 'primary'"
          @click="confirmReset"
        >
          执行 Reset
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { EllipsisVerticalOutline, RefreshOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import { getReflog } from "@/api/reflog";
import { listBranches, createBranch } from "@/api/branch";
import { resetTo } from "@/api/history";
import type { ReflogEntry } from "@/types/reflog";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();
const { resolveCurrentRepo } = useCurrentRepo();
const dialog = useDialog();

const repoPath = ref("");
const reference = ref("HEAD");
const locals = ref<string[]>([]);
const remotes = ref<string[]>([]);
const entries = ref<ReflogEntry[]>([]);
const loading = ref(false);

/** GF-13a：reflog 分页大小（与后端默认 200 对齐，超出部分走「加载更多」）。 */
const PAGE_SIZE = 200;
/** 当前请求的上限（等于已拉取的最大条数）。 */
const requestedMax = ref(PAGE_SIZE);
/** 可能还有更多（本次返回数打满上限）。 */
const hasMore = ref(false);
/** 已确认拉完全部（某次返回数小于请求上限）。 */
const allLoaded = ref(false);
const loadingMore = ref(false);

const referenceOptions = computed(() => {
  const opts: { label: string; value: string; type?: string }[] = [{ label: "HEAD", value: "HEAD" }];
  if (locals.value.length > 0) {
    opts.push({ label: "Local Branches", value: "__locals_group__", type: "group" });
    locals.value.forEach((b) => opts.push({ label: b, value: b }));
  }
  if (remotes.value.length > 0) {
    opts.push({ label: "Remote Branches", value: "__remotes_group__", type: "group" });
    remotes.value.forEach((r) => opts.push({ label: r, value: r }));
  }
  return opts;
});

const dropdownOptions = [
  { label: "View Commit", key: "view" },
  { label: "Create Branch Here", key: "branch" },
  { type: "divider", key: "d1" },
  { label: "Reset Here…", key: "reset" },
  { label: "Restore State（hard）", key: "restore" },
];

const viewDialog = reactive<{ show: boolean; entry: ReflogEntry | null }>({
  show: false,
  entry: null,
});
const resetDialog = reactive<{
  show: boolean;
  entry: ReflogEntry | null;
  mode: "soft" | "mixed" | "hard";
}>({ show: false, entry: null, mode: "mixed" });

onMounted(async () => {
  // F-14/F-17：query → 全局当前仓库 → 工作区首仓库兜底（SideNav 直达）。
  const repo = await resolveCurrentRepo();
  if (!repo) {
    message.warning("当前工作区没有可用仓库，请先在变更页扫描");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await loadBranchOptions();
  await load();
});

/** 分支列表只用于引用选择器选项，失败不阻塞 reflog 展示。 */
async function loadBranchOptions() {
  try {
    const overview = await listBranches(repoPath.value);
    locals.value = overview.locals.map((b) => b.name);
    remotes.value = overview.remotes.map((r) => r.name);
  } catch {
    // ignore
  }
}

// F-22：切换仓库后重置视图状态并重载（引用回退到 HEAD）；
// 分页状态由 load() 内的 resetPaging() 复位。
async function onRepoSwitch(path: string) {
  repoPath.value = path;
  entries.value = [];
  reference.value = "HEAD";
  await loadBranchOptions();
  await load();
}

/** GF-13a：重置分页状态（refresh / 切引用 / 历史操作后 load() 时调用）。 */
function resetPaging() {
  requestedMax.value = PAGE_SIZE;
  hasMore.value = false;
  allLoaded.value = false;
}

async function load() {
  loading.value = true;
  resetPaging();
  try {
    const list = await getReflog(repoPath.value, reference.value, requestedMax.value);
    entries.value = list;
    // 返回数打满上限 = 可能还有更多；短页 = 已到底。
    hasMore.value = list.length >= requestedMax.value;
    allLoaded.value = !hasMore.value;
  } catch (e) {
    entries.value = [];
    message.error("读取 reflog 失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

/** GF-13a：加载更多——reflog 只支持从 tip 起的 max 上限，故放大 max 重取
 *  （返回的是最新 max 条，与已展示的前缀一致，整体替换即可）。 */
async function loadMore() {
  if (loadingMore.value || loading.value || !hasMore.value) return;
  loadingMore.value = true;
  try {
    const nextMax = entries.value.length + PAGE_SIZE;
    const list = await getReflog(repoPath.value, reference.value, nextMax);
    entries.value = list;
    requestedMax.value = nextMax;
    hasMore.value = list.length >= nextMax;
    allLoaded.value = !hasMore.value;
  } catch (e) {
    message.error("加载更多失败: " + errMsg(e));
  } finally {
    loadingMore.value = false;
  }
}

function onAction(cmd: string, entry: ReflogEntry) {
  switch (cmd) {
    case "view":
      viewDialog.entry = entry;
      viewDialog.show = true;
      break;
    case "branch":
      handleCreateBranch(entry);
      break;
    case "reset":
      resetDialog.entry = entry;
      resetDialog.mode = "mixed";
      resetDialog.show = true;
      break;
    case "restore":
      handleRestore(entry);
      break;
  }
}

async function handleCreateBranch(entry: ReflogEntry) {
  try {
    const name = await prompt(dialog, {
      title: "Create Branch Here",
      content: `在 ${entry.selector}（${entry.newOid.slice(0, 7)} ${entry.commitMessage}）处创建分支：`,
      confirmText: "创建",
      cancelText: "取消",
      pattern: /^[^\s~^:?*[\]\\]+$/,
      patternError: "分支名不合法",
    });
    if (!name) return;
    await createBranch(repoPath.value, name, entry.newOid);
    message.success(`已创建分支 ${name}`);
  } catch (e) {
    if (e !== "cancel") message.error("创建分支失败: " + errMsg(e));
  }
}

/** Dangerous confirm content shared by Reset Here(hard) / Restore State. */
function dangerConfirmText(entry: ReflogEntry): string {
  return (
    `仓库：${repoPath.value}\n` +
    `目标：${entry.newOid.slice(0, 7)} ${entry.commitMessage}（${entry.selector}）\n\n` +
    `影响范围：HEAD、暂存区、工作区全部重置到该位置；未提交的更改将丢失。\n` +
    `保底：当前位置会留在 reflog 中，可再次回到本视图恢复。`
  );
}

async function confirmReset() {
  const entry = resetDialog.entry;
  if (!entry) return;
  const mode = resetDialog.mode;

  if (mode === "hard") {
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.error({
          title: "Reset --hard 确认（Dangerous）",
          content: dangerConfirmText(entry),
          positiveText: "确认 Hard Reset",
          negativeText: "取消",
          onPositiveClick: () => resolve(),
          onNegativeClick: () => reject("cancel"),
          onClose: () => reject("cancel"),
        });
      });
    } catch {
      return;
    }
  }

  resetDialog.show = false;
  try {
    await resetTo(repoPath.value, entry.newOid, mode);
    message.success(`已 Reset 到 ${entry.selector}（${mode}）`);
    await load();
  } catch (e) {
    message.error("Reset 失败: " + errMsg(e));
  }
}

/** Restore State = hard reset shortcut with Dangerous confirm (§46). */
async function handleRestore(entry: ReflogEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.error({
        title: "Restore State 确认（Dangerous）",
        content: dangerConfirmText(entry),
        positiveText: "恢复到此状态",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  try {
    await resetTo(repoPath.value, entry.newOid, "hard");
    message.success(`已恢复到 ${entry.selector}`);
    await load();
  } catch (e) {
    message.error("恢复失败: " + errMsg(e));
  }
}
</script>

<style scoped>
.reflog-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.reflog-header {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.repo-path {
  flex: 1;
  font-size: 14px;
  font-weight: 500;
  font-family: var(--gw-font-mono);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.reflog-body {
  flex: 1;
  overflow-y: auto;
  background: var(--gw-bg-panel);
}

.reflog-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
  padding: 6px 16px;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

/* GF-13a：条数提示 + 加载更多（页脚，不随列表滚动）。 */
.reflog-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--gw-space-3);
  padding: var(--gw-space-3);
  border-top: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.reflog-count {
  color: var(--gw-text-dim);
  font-size: 12px;
}

.selector {
  width: 140px;
  flex-shrink: 0;
  font-family: var(--gw-font-mono);
  color: var(--gw-accent);
}

.summary {
  width: 260px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-message {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--gw-font-mono);
  font-size: 12px;
  color: var(--gw-text);
}

.time {
  color: var(--gw-text-dim);
  font-size: 12px;
  flex-shrink: 0;
}

.danger-item {
  color: var(--gw-danger);
}

.reset-target {
  margin-bottom: 12px;
  font-size: 13px;
  color: var(--gw-text-dim);
}

.reset-modes {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}
</style>
