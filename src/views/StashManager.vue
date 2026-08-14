<template>
  <div class="stash-manager">
    <!-- Header -->
    <div class="stash-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <span class="repo-path">{{ repoPath }}</span>
      <el-button size="small" :loading="loading" @click="load">
        <el-icon><Refresh /></el-icon>
        刷新
      </el-button>
      <el-button size="small" type="primary" @click="saveDialog.show = true">
        <el-icon><Download /></el-icon>
        Stash Changes
      </el-button>
      <el-button
        size="small"
        type="danger"
        plain
        :disabled="entries.length === 0"
        @click="handleClear"
      >
        Clear All
      </el-button>
    </div>

    <!-- Stash list -->
    <div class="stash-body" v-loading="loading">
      <div v-for="entry in entries" :key="entry.oid" class="stash-row">
        <span class="stash-ref">stash@{{ "{" + entry.index + "}" }}</span>
        <span class="stash-message" :title="entry.message">{{ entry.message }}</span>
        <span class="stash-time">{{ entry.time }}</span>
        <div class="stash-actions">
          <el-button size="small" @click="handleApply(entry)">Apply</el-button>
          <el-button size="small" @click="handlePop(entry)">Pop</el-button>
          <el-button size="small" @click="openDiff(entry)">Show Diff</el-button>
          <el-dropdown trigger="click" @command="(cmd: string) => onMore(cmd, entry)">
            <el-button size="small" text>
              <el-icon><MoreFilled /></el-icon>
            </el-button>
            <template #dropdown>
              <el-dropdown-menu>
                <el-dropdown-item command="branch">Create Branch From Stash</el-dropdown-item>
                <el-dropdown-item command="drop" divided>
                  <span class="danger-item">Drop</span>
                </el-dropdown-item>
              </el-dropdown-menu>
            </template>
          </el-dropdown>
        </div>
      </div>
      <el-empty v-if="!loading && entries.length === 0" description="暂无 stash" :image-size="60" />
    </div>

    <!-- Stash save dialog -->
    <el-dialog v-model="saveDialog.show" title="Stash Changes" width="480px">
      <el-input
        v-model="saveDialog.message"
        placeholder="Stash 描述（可选）"
        style="margin-bottom: 12px"
      />
      <el-checkbox v-model="saveDialog.includeUntracked">
        包含未跟踪文件（include untracked）
      </el-checkbox>
      <template #footer>
        <el-button @click="saveDialog.show = false">取消</el-button>
        <el-button type="primary" :loading="saveDialog.loading" @click="handleSave">
          Stash
        </el-button>
      </template>
    </el-dialog>

    <!-- Stash diff dialog -->
    <el-dialog v-model="diffDialog.show" :title="`Stash Diff — stash@{${diffDialog.index}}`" width="80%" top="5vh">
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
          <el-empty v-if="diffDialog.files.length === 0" description="无文件差异" :image-size="40" />
        </div>
        <div class="file-diff">
          <UnifiedDiff v-if="diffDialog.selected" :file="diffDialog.selected" />
          <el-empty v-else description="选择文件查看 Diff" :image-size="40" />
        </div>
      </div>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, Download, MoreFilled, Refresh } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
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

onMounted(async () => {
  const repo = route.query.repo as string;
  if (!repo) {
    ElMessage.warning("未指定仓库路径");
    router.push({ name: "repository-list" });
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
    ElMessage.error("获取 stash 列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function goBack() {
  router.push({ name: "repository-list" });
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
    ElMessage.success(successMsg);
    await load();
  } catch (e) {
    ElMessage.error(errMsg(e));
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
    ElMessage.success("已 Stash");
    saveDialog.show = false;
    saveDialog.message = "";
    await load();
  } catch (e) {
    ElMessage.error("Stash 失败: " + errMsg(e));
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
    await ElMessageBox.confirm(
      `仓库：${repoPath.value}\n将丢弃 stash@{${entry.index}}（${entry.message}）。\n该 stash 的更改将从 stash 栈移除（必要时可经 reflog 尝试找回）。`,
      "Drop 确认（Warning）",
      { confirmButtonText: "丢弃", cancelButtonText: "取消", type: "warning" },
    );
  } catch {
    return;
  }
  await runOp(`已丢弃 stash@{${entry.index}}`, () => dropStash(repoPath.value, entry.index));
}

async function handleClear() {
  try {
    await ElMessageBox.confirm(
      `仓库：${repoPath.value}\n将清空全部 ${entries.value.length} 个 stash，不可批量撤销。`,
      "Clear All 确认（Warning）",
      { confirmButtonText: "全部清空", cancelButtonText: "取消", type: "warning" },
    );
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
      const { value: name } = await ElMessageBox.prompt(
        `从 stash@{${entry.index}} 创建分支（基于 stash 的基提交，检出后应用该 stash）：`,
        "Create Branch From Stash",
        {
          confirmButtonText: "创建",
          cancelButtonText: "取消",
          inputPattern: /^[^\s~^:?*[\]\\]+$/,
          inputErrorMessage: "分支名不合法",
        },
      );
      if (!name) return;
      await runOp(`已从 stash 创建分支 ${name}`, () =>
        branchFromStash(repoPath.value, name, entry.index),
      );
    } catch (e) {
      if (e !== "cancel") ElMessage.error("创建分支失败: " + errMsg(e));
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
    ElMessage.error("获取 stash diff 失败: " + errMsg(e));
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
