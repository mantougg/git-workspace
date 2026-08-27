<template>
  <div class="worktree-manager">
    <!-- Header -->
    <div class="wt-header">
      <span class="repo-path">{{ repoPath }}</span>
      <div class="wt-header-actions">
        <n-button size="small" :loading="loading" @click="load">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          刷新
        </n-button>
        <n-button size="small" type="primary" @click="openCreateDialog">
          <template #icon><n-icon><AddOutline /></n-icon></template>
          新建 Worktree
        </n-button>
      </div>
    </div>

    <!-- Worktree list -->
    <div class="wt-body">
      <n-spin :show="loading">
        <n-data-table
          v-if="worktrees.length > 0"
          :columns="columns"
          :data="worktrees"
          :bordered="false"
          :single-line="false"
        />
        <n-empty v-else-if="!loading" description="暂无 worktree" />
      </n-spin>
    </div>

    <!-- Create dialog -->
    <n-modal v-model:show="createDialog.show" preset="card" title="新建 Worktree" style="width: 560px">
      <n-form label-width="90">
        <n-form-item label="目标路径">
          <n-input v-model:value="createDialog.path" placeholder="worktree 目录路径" />
        </n-form-item>
        <n-form-item label="分支来源">
          <n-radio-group v-model:value="createDialog.mode">
            <n-radio value="new">新建分支</n-radio>
            <n-radio value="existing">现有分支</n-radio>
            <n-radio value="detached">游离 HEAD</n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item v-if="createDialog.mode === 'new'" label="新分支名">
          <n-input
            v-model:value="createDialog.newBranch"
            placeholder="基于当前 HEAD 创建"
          />
        </n-form-item>
        <n-form-item v-if="createDialog.mode === 'existing'" label="现有分支">
          <n-select v-model:value="createDialog.branch" filterable placeholder="选择分支" :options="branchOptions" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="createDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="createDialog.loading"
          @click="handleCreate"
        >
          创建
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { AddOutline, RefreshOutline } from "@vicons/ionicons5";
import { open as openPath } from "@tauri-apps/plugin-shell";
import { listWorktrees, createWorktree, removeWorktree } from "@/api/worktree";
import { listBranches } from "@/api/branch";
import type { WorktreeInfo } from "@/types/worktree";
import { errMsg } from "@/utils/error";
import { useMessage, useDialog, NTag, NButton } from "naive-ui";
import type { DataTableColumns } from "naive-ui";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();

const repoPath = ref("");
const worktrees = ref<WorktreeInfo[]>([]);
const loading = ref(false);
const localBranches = ref<string[]>([]);

const branchOptions = computed(() =>
  localBranches.value.map((b) => ({ label: b, value: b }))
);

const createDialog = ref({
  show: false,
  loading: false,
  path: "",
  mode: "new" as "new" | "existing" | "detached",
  newBranch: "",
  branch: "",
});

const columns = computed<DataTableColumns<WorktreeInfo>>(() => [
  {
    title: "名称",
    key: "name",
    minWidth: 160,
    render(row) {
      const tags = [];
      if (row.isMain) {
        tags.push(h(NTag, { size: "small", type: "success", bordered: false }, { default: () => "主仓库" }));
      }
      if (row.isLocked) {
        tags.push(h(NTag, { size: "small", type: "error", bordered: false }, { default: () => "锁定" }));
      }
      return h("span", {}, [
        h("span", { class: "wt-name" }, row.name),
        ...tags,
      ]);
    },
  },
  {
    title: "分支",
    key: "branch",
    minWidth: 140,
    render(row) {
      if (row.branch) {
        return h(NTag, { size: "small" }, { default: () => row.branch });
      }
      return h(NTag, { size: "small", type: "info", bordered: false }, { default: () => "游离 HEAD" });
    },
  },
  {
    title: "路径",
    key: "path",
    minWidth: 260,
    render(row) {
      return h("span", { class: "wt-path" }, row.path);
    },
  },
  {
    title: "状态",
    key: "isDirty",
    width: 110,
    render(row) {
      if (row.isDirty) {
        return h(NTag, { size: "small", type: "warning", bordered: false }, { default: () => "有未提交变更" });
      }
      return h(NTag, { size: "small", type: "success", bordered: false }, { default: () => "干净" });
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 300,
    fixed: "right",
    render(row) {
      const buttons = [
        h(NButton, { size: "small", text: true, onClick: () => viewGraph(row) }, { default: () => "Graph" }),
        h(NButton, { size: "small", text: true, onClick: () => viewDiff(row) }, { default: () => "Diff" }),
        h(NButton, { size: "small", text: true, onClick: () => openFolder(row) }, { default: () => "打开目录" }),
      ];
      if (!row.isMain) {
        buttons.push(
          h(NButton, { size: "small", text: true, type: "error", onClick: () => handleRemove(row) }, { default: () => "移除" })
        );
      }
      return h("div", { style: "display: flex; gap: 4px;" }, buttons);
    },
  },
]);

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
    worktrees.value = await listWorktrees(repoPath.value);
  } catch (e) {
    message.error("获取 worktree 列表失败: " + errMsg(e));
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
    message.warning("请输入目标路径");
    return;
  }
  if (d.mode === "new" && !d.newBranch.trim()) {
    message.warning("请输入新分支名");
    return;
  }
  if (d.mode === "existing" && !d.branch) {
    message.warning("请选择分支");
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
    message.success("Worktree 已创建");
    d.show = false;
    await load();
  } catch (e) {
    message.error("创建失败: " + errMsg(e));
  } finally {
    d.loading = false;
  }
}

/** Remove with the §46 Warning flow: dirty worktrees need a second confirm. */
async function handleRemove(row: WorktreeInfo) {
  try {
    await new Promise<void>((resolve, reject) => {
      dialog.warning({
        title: "移除 Worktree",
        content: `确定移除 worktree「${row.name}」吗？\n目录：${row.path}`,
        positiveText: "移除",
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
    await removeWorktree(repoPath.value, row.name, false);
    message.success("已移除");
    await load();
  } catch (e) {
    const msg = errMsg(e);
    if (!msg.includes("未提交变更")) {
      message.error("移除失败: " + msg);
      return;
    }
    // Dirty worktree: §46 Warning — explicit second confirmation, then force.
    try {
      await new Promise<void>((resolve, reject) => {
        dialog.error({
          title: "警告：Worktree 含未提交变更",
          content: `${msg}\n\n确定要强制移除吗？未提交变更将丢失（可用 reflog/stash 保底）。`,
          positiveText: "强制移除",
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
      await removeWorktree(repoPath.value, row.name, true);
      message.success("已强制移除");
      await load();
    } catch (e2) {
      message.error("移除失败: " + errMsg(e2));
    }
  }
}

/** Open the worktree directory in the OS file manager. */
async function openFolder(row: WorktreeInfo) {
  try {
    await openPath(row.path);
  } catch (e) {
    message.error("打开目录失败: " + errMsg(e));
  }
}

/** "Checkout" = switch the app's repo context to this worktree. */
function viewGraph(row: WorktreeInfo) {
  router.push({ name: "git-graph", query: { repo: row.path } });
}

function viewDiff(row: WorktreeInfo) {
  router.push({ name: "diff-viewer", query: { repo: row.path } });
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
