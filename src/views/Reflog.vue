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
      <n-radio-group v-model:value="resetDialog.mode" class="reset-modes" :disabled="resetDialog.restore">
        <n-radio value="soft">soft — 仅移动 HEAD，保留暂存区与工作区</n-radio>
        <n-radio value="mixed">mixed — 移动 HEAD + 重置暂存区，保留工作区</n-radio>
        <n-radio value="hard">hard — 重置全部，丢弃未提交更改（危险）</n-radio>
      </n-radio-group>
      <!-- GF-17：结构化预演 —— 「操作会发生什么」（Roadmap §46） -->
      <n-spin :show="resetDialog.previewLoading" size="small" class="reset-preview">
        <div v-if="resetDialog.previewError" class="reset-preview-error">
          预演加载失败：{{ resetDialog.previewError }}（仍可执行，执行时会再次校验）
        </div>
        <template v-else-if="resetDialog.preview">
          <div class="reset-preview-line">
            将丢弃
            <strong :class="{ 'danger-text': resetDialog.preview.discardedCount > 0 }">{{
              resetDialog.preview.discardedCount
            }}</strong>
            个提交
            <span
              v-if="resetDialog.preview.discardedCommits.length < resetDialog.preview.discardedCount"
              class="dim-text"
            >
              （仅显示前 {{ resetDialog.preview.discardedCommits.length }} 个）
            </span>
          </div>
          <ul v-if="resetDialog.preview.discardedCommits.length" class="reset-preview-list">
            <li v-for="c in resetDialog.preview.discardedCommits" :key="c.oid">
              <span class="mono">{{ c.shortOid }}</span>
              <span class="reset-preview-msg">{{ c.summary }}</span>
            </li>
          </ul>
          <div class="reset-preview-line">
            将丢弃
            <strong :class="{ 'danger-text': resetDialog.preview.unrecoverable }">{{
              resetDialog.preview.lostChangesCount
            }}</strong>
            个未提交变更
            <span v-if="resetDialog.preview.unrecoverable" class="danger-text">
              （不可恢复，reflog 无法找回）
            </span>
          </div>
          <ul v-if="resetDialog.preview.lostFileChanges.length" class="reset-preview-list">
            <li
              v-for="f in resetDialog.preview.lostFileChanges"
              :key="f.path"
              :class="{ 'danger-text': resetDialog.preview.unrecoverable }"
            >
              <span class="mono">{{ f.path }}</span>
              <span class="dim-text">（{{ f.status }}）</span>
            </li>
          </ul>
          <div v-if="resetDialog.mode === 'soft'" class="dim-text">
            soft：被丢弃提交的内容会进入暂存区，文件变更不丢失。
          </div>
          <div v-else-if="resetDialog.mode === 'mixed'" class="dim-text">
            mixed：暂存区被重置，未提交内容保留在工作区。
          </div>
        </template>
      </n-spin>
      <template #footer>
        <n-button
          @click="
            resetDialog.show = false;
            resetDialog.restore = false;
          "
          >取消</n-button
        >
        <n-button
          :type="resetDialog.mode === 'hard' ? 'error' : 'primary'"
          @click="confirmReset"
        >
          {{ resetDialog.restore ? "恢复到此状态" : "执行 Reset" }}
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useCurrentRepo } from "@/composables/useCurrentRepo";
import RepoSwitcher from "@/components/shell/RepoSwitcher.vue";
import { EllipsisVerticalOutline, RefreshOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import { getReflog } from "@/api/reflog";
import { listBranches, createBranch } from "@/api/branch";
import { resetTo } from "@/api/history";
import { previewReset } from "@/api/preview";
import type { ReflogEntry } from "@/types/reflog";
import type { ResetPreview } from "@/types/preview";
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
  /** true = Restore State 快捷入口（锁定 hard 模式，仅补结构化预演）。 */
  restore: boolean;
  /** GF-17：结构化预演（只读命令 preview_reset 的结果）。 */
  preview: ResetPreview | null;
  previewLoading: boolean;
  previewError: string;
}>({
  show: false,
  entry: null,
  mode: "mixed",
  restore: false,
  preview: null,
  previewLoading: false,
  previewError: "",
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
      resetDialog.restore = false;
      resetDialog.show = true;
      loadResetPreview();
      break;
    case "restore":
      // GF-17：Restore State 仍默认 hard，但改走带结构化预演的确认框
      // （原来是纯文案 dialog.error）。
      resetDialog.entry = entry;
      resetDialog.mode = "hard";
      resetDialog.restore = true;
      resetDialog.show = true;
      loadResetPreview();
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

/** GF-17：加载 reset 结构化预演（只读；失败不阻塞确认流，执行时会再次校验）。 */
async function loadResetPreview() {
  const entry = resetDialog.entry;
  if (!entry || !repoPath.value) return;
  resetDialog.previewLoading = true;
  resetDialog.previewError = "";
  try {
    resetDialog.preview = await previewReset(repoPath.value, entry.newOid, resetDialog.mode);
  } catch (e) {
    resetDialog.preview = null;
    resetDialog.previewError = errMsg(e);
  } finally {
    resetDialog.previewLoading = false;
  }
}

// 模式影响「不可恢复」判定（hard + 未提交变更），切换时重取预演。
watch(
  () => resetDialog.mode,
  () => {
    if (resetDialog.show) loadResetPreview();
  },
);

/** 预演事实 + 恢复提示（Dangerous 确认正文；无预演时退回纯文案）。 */
function dangerConfirmText(entry: ReflogEntry, preview: ResetPreview | null): string {
  const target = `目标：${entry.newOid.slice(0, 7)} ${entry.commitMessage}（${entry.selector}）`;
  if (!preview) {
    return (
      `仓库：${repoPath.value}\n` +
      `${target}\n\n` +
      `影响范围：HEAD、暂存区、工作区全部重置到该位置；未提交的更改将丢失。\n` +
      `保底：当前位置会留在 reflog 中，可再次回到本视图恢复。`
    );
  }
  return (
    `仓库：${repoPath.value}\n` +
    `当前分支：${preview.branch}\n` +
    `${target}\n` +
    `将丢弃 ${preview.discardedCount} 个提交（${preview.discardedCommits
      .slice(0, 3)
      .map((c) => c.shortOid)
      .join(", ")}${preview.discardedCount > 3 ? " 等" : ""}）\n` +
    (preview.lostChangesCount > 0
      ? `将丢弃 ${preview.lostChangesCount} 个未提交变更${preview.unrecoverable ? "（不可恢复，reflog 无法找回）" : ""}`
      : "工作区无未提交变更") +
    `\n\n影响范围：HEAD、暂存区、工作区全部重置到该位置；未提交的更改将丢失。\n` +
    `保底：当前位置会留在 reflog 中，可再次回到本视图恢复。`
  );
}

async function confirmReset() {
  const entry = resetDialog.entry;
  if (!entry) return;
  const mode = resetDialog.mode;
  const restore = resetDialog.restore;
  const preview = resetDialog.preview;

  if (mode === "hard") {
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.error({
          title: restore ? "Restore State 确认（Dangerous）" : "Reset --hard 确认（Dangerous）",
          content: dangerConfirmText(entry, preview),
          positiveText: restore ? "恢复到此状态" : "确认 Hard Reset",
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
  resetDialog.restore = false;
  try {
    await resetTo(repoPath.value, entry.newOid, mode);
    message.success(restore ? `已恢复到 ${entry.selector}` : `已 Reset 到 ${entry.selector}（${mode}）`);
    await load();
  } catch (e) {
    message.error(restore ? "恢复失败: " + errMsg(e) : "Reset 失败: " + errMsg(e));
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

/* GF-17：结构化预演区块（Roadmap §46 Repository/Branch/Files/Potential Data Loss） */
.reset-preview {
  display: block;
  margin-top: var(--gw-space-3);
  padding-top: var(--gw-space-3);
  border-top: 1px solid var(--gw-border);
  min-height: 24px;
}

.reset-preview-line {
  font-size: 13px;
  line-height: 1.8;
}

.reset-preview-list {
  margin: var(--gw-space-1) 0;
  padding-left: var(--gw-space-4);
  max-height: 140px;
  overflow-y: auto;
  font-size: 12px;
  line-height: 1.7;
}

.reset-preview-msg {
  margin-left: var(--gw-space-2);
  color: var(--gw-text-dim);
}

.reset-preview-error {
  font-size: 12px;
  color: var(--gw-warning);
}

.danger-text {
  color: var(--gw-danger);
}

.dim-text {
  color: var(--gw-text-dim);
}

.mono {
  font-family: var(--gw-font-mono);
}
</style>
