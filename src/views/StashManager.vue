<template>
  <div class="stash-manager">
    <!-- Header -->
    <div class="stash-header">
      <span class="repo-path">{{ repoPath }}</span>
      <n-button size="small" :loading="loading" @click="load">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
      <n-button size="small" type="primary" @click="saveDialog.show = true">
        <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
        Stash Changes
      </n-button>
      <n-button
        size="small"
        type="error"
        :disabled="entries.length === 0"
        @click="handleClear"
      >
        Clear All
      </n-button>
    </div>

    <!-- Stash list -->
    <n-spin :show="loading">
      <div class="stash-body">
        <div v-for="entry in entries" :key="entry.oid" class="stash-row">
          <span class="stash-ref">stash@{{ "{" + entry.index + "}" }}</span>
          <span class="stash-message" :title="entry.message">{{ entry.message }}</span>
          <span class="stash-time">{{ entry.time }}</span>
          <div class="stash-actions">
            <n-button size="small" @click="handleApply(entry)">Apply</n-button>
            <n-button size="small" @click="handlePop(entry)">Pop</n-button>
            <n-button size="small" @click="openDiff(entry)">Show Diff</n-button>
            <n-dropdown trigger="click" :options="moreDropdownOptions" @select="(key: string) => onMore(key, entry)">
              <n-button size="small" text>
                <template #icon><n-icon><EllipsisVerticalOutline /></n-icon></template>
              </n-button>
            </n-dropdown>
          </div>
        </div>
        <n-empty v-if="!loading && entries.length === 0" description="暂无 stash" />
      </div>
    </n-spin>

    <!-- Stash save dialog -->
    <n-modal v-model:show="saveDialog.show" preset="card" title="Stash Changes" style="width: 480px">
      <n-input
        v-model:value="saveDialog.message"
        placeholder="Stash 描述（可选）"
        style="margin-bottom: 12px"
      />
      <n-checkbox v-model:checked="saveDialog.includeUntracked">
        包含未跟踪文件（include untracked）
      </n-checkbox>
      <template #footer>
        <n-button @click="saveDialog.show = false">取消</n-button>
        <n-button type="primary" :loading="saveDialog.loading" @click="handleSave">
          Stash
        </n-button>
      </template>
    </n-modal>

    <!-- Stash diff dialog -->
    <n-modal v-model:show="diffDialog.show" preset="card" :title="`Stash Diff — stash@{${diffDialog.index}}`" style="width: 80%">
      <div class="stash-diff">
        <div class="file-list">
          <div
            v-for="f in diffDialog.files"
            :key="f.newPath"
            :class="['file-item', { active: diffDialog.selected?.newPath === f.newPath }]"
            @click="diffDialog.selected = f"
          >
            <span :class="['file-status-icon', f.status]">{{ statusIcon(f.status) }}</span>
            <span class="file-name">{{ f.newPath }}</span>
          </div>
          <n-empty v-if="diffDialog.files.length === 0" description="无文件差异" />
        </div>
        <div class="file-diff">
          <UnifiedDiff v-if="diffDialog.selected" :file="diffDialog.selected" />
          <n-empty v-else description="选择文件查看 Diff" />
        </div>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { CloudUploadOutline, EllipsisVerticalOutline, RefreshOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import {
  applyStash,
  branchFromStash,
  clearStashes,
  dropStash,
  getStashDiff,
  listStashes,
  popStash,
  stashChanges,
} from "@/api/stash";
import type { StashEntry } from "@/types/stash";
import type { FileDiff } from "@/types/git";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();

const repoPath = ref("");
const entries = ref<StashEntry[]>([]);
const loading = ref(false);

const saveDialog = reactive({ show: false, message: "", includeUntracked: false, loading: false });
const diffDialog = reactive<{
  show: boolean;
  index: number;
  files: FileDiff[];
  selected: FileDiff | null;
}>({ show: false, index: 0, files: [], selected: null });

const moreDropdownOptions = [
  { label: "Create Branch From Stash", key: "branch" },
  { type: "divider", key: "d1" },
  { label: "Drop", key: "drop" },
];

onMounted(async () => {
  const repo = route.query.repo as string;
  if (!repo) {
    message.warning("未指定仓库路径");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await load();
});

async function load() {
  loading.value = true;
  try {
    entries.value = await listStashes(repoPath.value);
  } catch (e) {
    message.error("获取 stash 列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function statusIcon(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "A";
    case "deleted":
      return "D";
    case "modified":
      return "M";
    case "renamed":
      return "R";
    default:
      return "?";
  }
}

/** Run an op, toast, reload on success. */
async function runOp(successMsg: string, op: () => Promise<unknown>) {
  try {
    await op();
    message.success(successMsg);
    await load();
  } catch (e) {
    message.error(errMsg(e));
  }
}

async function handleSave() {
  saveDialog.loading = true;
  try {
    await stashChanges(
      repoPath.value,
      saveDialog.message || undefined,
      saveDialog.includeUntracked,
    );
    message.success("已 Stash");
    saveDialog.show = false;
    saveDialog.message = "";
    await load();
  } catch (e) {
    message.error("Stash 失败: " + errMsg(e));
  } finally {
    saveDialog.loading = false;
  }
}

async function handleApply(entry: StashEntry) {
  await runOp(`已应用 stash@{${entry.index}}`, () => applyStash(repoPath.value, entry.index));
}

async function handlePop(entry: StashEntry) {
  await runOp(`已 Pop stash@{${entry.index}}`, () => popStash(repoPath.value, entry.index));
}

/** Drop / Clear are Warning-level (§46): confirm with impact + recovery hint. */
async function handleDrop(entry: StashEntry) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Drop 确认（Warning）",
        content: `仓库：${repoPath.value}\n将丢弃 stash@{${entry.index}}（${entry.message}）。\n该 stash 的更改将从 stash 栈移除（必要时可经 reflog 尝试找回）。`,
        positiveText: "丢弃",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  await runOp(`已丢弃 stash@{${entry.index}}`, () => dropStash(repoPath.value, entry.index));
}

async function handleClear() {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "Clear All 确认（Warning）",
        content: `仓库：${repoPath.value}\n将清空全部 ${entries.value.length} 个 stash，不可批量撤销。`,
        positiveText: "全部清空",
        negativeText: "取消",
        onPositiveClick: () => resolve(),
        onNegativeClick: () => reject("cancel"),
        onClose: () => reject("cancel"),
      });
    });
  } catch {
    return;
  }
  await runOp("已清空 stash 栈", async () => {
    await clearStashes(repoPath.value);
  });
}

async function onMore(cmd: string, entry: StashEntry) {
  if (cmd === "drop") {
    await handleDrop(entry);
    return;
  }
  if (cmd === "branch") {
    try {
      const name = await prompt(dialog, {
        title: "Create Branch From Stash",
        content: `从 stash@{${entry.index}} 创建分支（基于 stash 的基提交，检出后应用该 stash）：`,
        confirmText: "创建",
        cancelText: "取消",
        pattern: /^[^\s~^:?*[\]\\]+$/,
        patternError: "分支名不合法",
      });
      if (!name) return;
      await runOp(`已从 stash 创建分支 ${name}`, () =>
        branchFromStash(repoPath.value, name, entry.index),
      );
    } catch (e) {
      if (e !== "cancel") message.error("创建分支失败: " + errMsg(e));
    }
  }
}

async function openDiff(entry: StashEntry) {
  try {
    const files = await getStashDiff(repoPath.value, entry.index);
    diffDialog.index = entry.index;
    diffDialog.files = files;
    diffDialog.selected = files[0] ?? null;
    diffDialog.show = true;
  } catch (e) {
    message.error("获取 stash diff 失败: " + errMsg(e));
  }
}
</script>

<style scoped>
.stash-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.stash-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid #ebeef5;
  background: #fff;
}

.repo-path {
  flex: 1;
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stash-body {
  flex: 1;
  overflow-y: auto;
  background: #fff;
}

.stash-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid #f5f5f5;
  font-size: 13px;
}

.stash-ref {
  width: 100px;
  flex-shrink: 0;
  font-family: "Cascadia Code", Consolas, monospace;
  color: #409eff;
}

.stash-message {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.stash-time {
  color: #909399;
  font-size: 12px;
  flex-shrink: 0;
}

.stash-actions {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.danger-item {
  color: #f56c6c;
}

.stash-diff {
  display: flex;
  height: 55vh;
  border: 1px solid #ebeef5;
}

.file-list {
  width: 260px;
  border-right: 1px solid #ebeef5;
  overflow-y: auto;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  cursor: pointer;
  font-size: 13px;
  border-bottom: 1px solid #f5f5f5;
}

.file-item:hover {
  background: #f5f7fa;
}

.file-item.active {
  background: #ecf5ff;
}

.file-status-icon {
  width: 16px;
  text-align: center;
  font-weight: bold;
  flex-shrink: 0;
}

.file-status-icon.added,
.file-status-icon.untracked {
  color: #67c23a;
}

.file-status-icon.deleted {
  color: #f56c6c;
}

.file-status-icon.modified {
  color: #e6a23c;
}

.file-status-icon.renamed {
  color: #909399;
}

.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-diff {
  flex: 1;
  overflow: hidden;
}
</style>
