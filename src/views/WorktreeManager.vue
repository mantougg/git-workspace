<template>
  <div class="worktree-manager">
    <!-- Header -->
    <div class="wt-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <span class="repo-path">{{ repoPath }}</span>
      <div class="wt-header-actions">
        <el-button size="small" :loading="loading" @click="load">
          <el-icon><Refresh /></el-icon>
          刷新
        </el-button>
        <el-button size="small" type="primary" @click="openCreateDialog">
          <el-icon><Plus /></el-icon>
          新建 Worktree
        </el-button>
      </div>
    </div>

    <!-- Worktree list -->
    <div class="wt-body" v-loading="loading">
      <el-table v-if="worktrees.length > 0" :data="worktrees" style="width: 100%">
        <el-table-column label="名称" min-width="160">
          <template #default="{ row }">
            <span class="wt-name">{{ row.name }}</span>
            <el-tag v-if="row.isMain" size="small" type="success" effect="plain">
              主仓库
            </el-tag>
            <el-tag v-if="row.isLocked" size="small" type="danger" effect="plain">
              锁定
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="分支" min-width="140">
          <template #default="{ row }">
            <el-tag v-if="row.branch" size="small">{{ row.branch }}</el-tag>
            <el-tag v-else size="small" type="info" effect="plain">
              游离 HEAD
            </el-tag>
          </template>
        </el-table-column>
        <el-table-column label="路径" min-width="260">
          <template #default="{ row }">
            <span class="wt-path">{{ row.path }}</span>
          </template>
        </el-table-column>
        <el-table-column label="状态" width="110">
          <template #default="{ row }">
            <el-tag v-if="row.isDirty" size="small" type="warning" effect="plain">
              有未提交变更
            </el-tag>
            <el-tag v-else size="small" type="success" effect="plain">干净</el-tag>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="300" fixed="right">
          <template #default="{ row }">
            <el-button size="small" text @click="viewGraph(row)">Graph</el-button>
            <el-button size="small" text @click="viewDiff(row)">Diff</el-button>
            <el-button size="small" text @click="openFolder(row)">
              打开目录
            </el-button>
            <el-button
              v-if="!row.isMain"
              size="small"
              text
              type="danger"
              @click="handleRemove(row)"
            >
              移除
            </el-button>
          </template>
        </el-table-column>
      </el-table>
      <el-empty v-else-if="!loading" description="暂无 worktree" />
    </div>

    <!-- Create dialog -->
    <el-dialog v-model="createDialog.show" title="新建 Worktree" width="560px">
      <el-form label-width="90px">
        <el-form-item label="目标路径">
          <el-input v-model="createDialog.path" placeholder="worktree 目录路径" />
        </el-form-item>
        <el-form-item label="分支来源">
          <el-radio-group v-model="createDialog.mode">
            <el-radio value="new">新建分支</el-radio>
            <el-radio value="existing">现有分支</el-radio>
            <el-radio value="detached">游离 HEAD</el-radio>
          </el-radio-group>
        </el-form-item>
        <el-form-item v-if="createDialog.mode === 'new'" label="新分支名">
          <el-input
            v-model="createDialog.newBranch"
            placeholder="基于当前 HEAD 创建"
          />
        </el-form-item>
        <el-form-item v-if="createDialog.mode === 'existing'" label="现有分支">
          <el-select v-model="createDialog.branch" filterable placeholder="选择分支">
            <el-option
              v-for="b in localBranches"
              :key="b"
              :label="b"
              :value="b"
            />
          </el-select>
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="createDialog.show = false">取消</el-button>
        <el-button
          type="primary"
          :loading="createDialog.loading"
          @click="handleCreate"
        >
          创建
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, Plus, Refresh } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { open as openPath } from "@tauri-apps/plugin-shell";
import { listWorktrees, createWorktree, removeWorktree } from "@/api/worktree";
import { listBranches } from "@/api/branch";
import type { WorktreeInfo } from "@/types/worktree";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();

const repoPath = ref("");
const worktrees = ref<WorktreeInfo[]>([]);
const loading = ref(false);
const localBranches = ref<string[]>([]);

const createDialog = ref({
  show: false,
  loading: false,
  path: "",
  mode: "new" as "new" | "existing" | "detached",
  newBranch: "",
  branch: "",
});

onMounted(async () => {
  const repo = route.query.repo as string;
  if (!repo) {
    ElMessage.warning("未指定仓库路径");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  await load();
});

async function load() {
  loading.value = true;
  try {
    worktrees.value = await listWorktrees(repoPath.value);
  } catch (e) {
    ElMessage.error("获取 worktree 列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

/** Default target path: sibling directory named `<repo>-<branch|wt>`. */
function defaultPath(): string {
  const segs = repoPath.value.split(/[/\\]/).filter(Boolean);
  const name = segs.pop() ?? "repo";
  const parent = repoPath.value.slice(0, repoPath.value.length - name.length);
  const suffix =
    createDialog.value.mode === "new" && createDialog.value.newBranch.trim()
      ? createDialog.value.newBranch.trim().replace(/[/\\]/g, "-")
      : "wt";
  return `${parent}${name}-${suffix}`;
}

async function openCreateDialog() {
  createDialog.value.newBranch = "";
  createDialog.value.branch = "";
  try {
    const overview = await listBranches(repoPath.value);
    localBranches.value = overview.locals.map((b) => b.name);
  } catch {
    localBranches.value = [];
  }
  createDialog.value.path = defaultPath();
  createDialog.value.show = true;
}

async function handleCreate() {
  const d = createDialog.value;
  if (!d.path.trim()) {
    ElMessage.warning("请输入目标路径");
    return;
  }
  if (d.mode === "new" && !d.newBranch.trim()) {
    ElMessage.warning("请输入新分支名");
    return;
  }
  if (d.mode === "existing" && !d.branch) {
    ElMessage.warning("请选择分支");
    return;
  }
  d.loading = true;
  try {
    await createWorktree(
      repoPath.value,
      d.path.trim(),
      d.mode === "existing" ? d.branch : null,
      d.mode === "new" ? d.newBranch.trim() : null,
    );
    ElMessage.success("Worktree 已创建");
    d.show = false;
    await load();
  } catch (e) {
    ElMessage.error("创建失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}

/** Remove with the §46 Warning flow: dirty worktrees need a second confirm. */
async function handleRemove(row: WorktreeInfo) {
  try {
    await ElMessageBox.confirm(
      `确定移除 worktree「${row.name}」吗？\n目录：${row.path}`,
      "移除 Worktree",
      { type: "warning", confirmButtonText: "移除", cancelButtonText: "取消" },
    );
  } catch {
    return;
  }
  try {
    await removeWorktree(repoPath.value, row.name, false);
    ElMessage.success("已移除");
    await load();
  } catch (e) {
    const msg = errMsg(e);
    if (!msg.includes("未提交变更")) {
      ElMessage.error("移除失败: " + msg);
      return;
    }
    // Dirty worktree: §46 Warning — explicit second confirmation, then force.
    try {
      await ElMessageBox.confirm(
        `${msg}\n\n确定要强制移除吗？未提交变更将丢失（可用 reflog/stash 保底）。`,
        "警告：Worktree 含未提交变更",
        { type: "error", confirmButtonText: "强制移除", cancelButtonText: "取消" },
      );
    } catch {
      return;
    }
    try {
      await removeWorktree(repoPath.value, row.name, true);
      ElMessage.success("已强制移除");
      await load();
    } catch (e2) {
      ElMessage.error("移除失败: " + errMsg(e2));
    }
  }
}

/** Open the worktree directory in the OS file manager. */
async function openFolder(row: WorktreeInfo) {
  try {
    await openPath(row.path);
  } catch (e) {
    ElMessage.error("打开目录失败: " + errMsg(e));
  }
}

/** "Checkout" = switch the app's repo context to this worktree. */
function viewGraph(row: WorktreeInfo) {
  router.push({ name: "git-graph", query: { repo: row.path } });
}

function viewDiff(row: WorktreeInfo) {
  router.push({ name: "diff-viewer", query: { repo: row.path } });
}

function goBack() {
  router.push({ name: "changes" });
}
</script>

<style scoped>
.worktree-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.wt-header {
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
}

.wt-header-actions {
  display: flex;
  gap: 8px;
}

.wt-body {
  flex: 1;
  overflow: auto;
  padding: 8px 16px;
}

.wt-name {
  font-weight: 500;
  margin-right: 6px;
}

.wt-path {
  font-family: monospace;
  font-size: 12px;
  color: #606266;
}
</style>
