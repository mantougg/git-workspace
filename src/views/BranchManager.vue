<template>
  <div class="branch-manager">
    <!-- Header -->
    <div class="branch-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <div class="repo-info">
        <span class="repo-path">{{ repoPath }}</span>
        <el-tag v-if="overview?.current" size="small" type="success">
          {{ overview.current }}
        </el-tag>
        <el-tag v-else-if="overview" size="small" type="warning">HEAD 游离</el-tag>
      </div>
      <el-button size="small" :loading="loading" @click="load">
        <el-icon><Refresh /></el-icon>
        刷新
      </el-button>
      <el-button size="small" type="primary" @click="handleCreate">
        <el-icon><Plus /></el-icon>
        新建分支
      </el-button>
      <el-button size="small" @click="openCompare()">
        <el-icon><Switch /></el-icon>
        Compare
      </el-button>
    </div>

    <div class="branch-body" v-loading="loading">
      <template v-if="overview">
        <!-- Local branches -->
        <div class="section">
          <div class="section-title">Local Branches（{{ overview.locals.length }}）</div>
          <div
            v-for="b in overview.locals"
            :key="b.name"
            :class="['branch-row', { current: b.isCurrent }]"
          >
            <span class="branch-name">
              {{ b.name }}
              <el-tag v-if="b.isCurrent" size="small" type="success" effect="plain">当前</el-tag>
            </span>
            <span class="branch-track">
              <template v-if="b.upstream">
                <span class="upstream">{{ b.upstream }}</span>
                <span v-if="b.ahead > 0" class="ahead">↑{{ b.ahead }}</span>
                <span v-if="b.behind > 0" class="behind">↓{{ b.behind }}</span>
              </template>
              <span v-else class="no-upstream">无上游</span>
            </span>
            <span class="branch-commit" :title="b.lastCommitOid">
              {{ shortOid(b.lastCommitOid) }} {{ b.lastCommitMessage }}
            </span>
            <el-dropdown trigger="click" @command="(cmd: string) => handleLocalCommand(cmd, b)">
              <el-button size="small" text>
                <el-icon><MoreFilled /></el-icon>
              </el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item v-if="!b.isCurrent" command="checkout">Checkout</el-dropdown-item>
                  <el-dropdown-item command="rename">Rename</el-dropdown-item>
                  <el-dropdown-item command="set-upstream">Set Upstream</el-dropdown-item>
                  <el-dropdown-item v-if="b.isCurrent" command="pull">Pull（--ff-only）</el-dropdown-item>
                  <el-dropdown-item command="push">Push</el-dropdown-item>
                  <el-dropdown-item command="compare">Compare</el-dropdown-item>
                  <el-dropdown-item disabled title="Merge 将在 T-15 提供">Merge（T-15）</el-dropdown-item>
                  <el-dropdown-item disabled title="Rebase 将在 T-15 提供">Rebase（T-15）</el-dropdown-item>
                  <el-dropdown-item v-if="!b.isCurrent" command="delete" divided>
                    <span class="danger-item">Delete</span>
                  </el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </div>
          <el-empty v-if="overview.locals.length === 0" description="无本地分支" :image-size="40" />
        </div>

        <!-- Remote branches -->
        <div class="section">
          <div class="section-title">Remote Branches（{{ overview.remotes.length }}）</div>
          <div v-for="r in overview.remotes" :key="r.name" class="branch-row">
            <span class="branch-name">{{ r.name }}</span>
            <span class="branch-track" />
            <span class="branch-commit" :title="r.lastCommitOid">
              {{ shortOid(r.lastCommitOid) }} {{ r.lastCommitMessage }}
            </span>
            <el-dropdown trigger="click" @command="(cmd: string) => handleRemoteCommand(cmd, r)">
              <el-button size="small" text>
                <el-icon><MoreFilled /></el-icon>
              </el-button>
              <template #dropdown>
                <el-dropdown-menu>
                  <el-dropdown-item command="track">Track（检出为本地分支）</el-dropdown-item>
                  <el-dropdown-item command="compare">Compare</el-dropdown-item>
                </el-dropdown-menu>
              </template>
            </el-dropdown>
          </div>
          <el-empty v-if="overview.remotes.length === 0" description="无远程分支" :image-size="40" />
        </div>

        <!-- Tags -->
        <div class="section">
          <div class="section-title">Tags（{{ overview.tags.length }}）</div>
          <div v-for="t in overview.tags" :key="t.name" class="branch-row">
            <span class="branch-name">{{ t.name }}</span>
            <span class="branch-track tag-message" :title="t.message ?? ''">{{ t.message ?? "" }}</span>
            <span class="branch-commit" :title="t.targetOid">{{ shortOid(t.targetOid) }}</span>
            <span />
          </div>
          <el-empty v-if="overview.tags.length === 0" description="无标签" :image-size="40" />
        </div>
      </template>
    </div>

    <!-- Compare dialog -->
    <el-dialog v-model="compare.show" title="Branch Compare" width="80%" top="5vh">
      <div class="compare-form">
        <el-select v-model="compare.base" filterable placeholder="Base（基准）" style="width: 240px">
          <el-option v-for="o in revisionOptions" :key="o" :label="o" :value="o" />
        </el-select>
        <span class="compare-arrow">⇄</span>
        <el-select v-model="compare.other" filterable placeholder="Other（对比）" style="width: 240px">
          <el-option v-for="o in revisionOptions" :key="o" :label="o" :value="o" />
        </el-select>
        <el-button
          type="primary"
          :loading="compare.loading"
          :disabled="!compare.base || !compare.other"
          @click="runCompare"
        >
          比较
        </el-button>
      </div>

      <div v-if="compare.result" class="compare-result">
        <div class="compare-summary">
          <el-tag type="success">领先 {{ compare.result.ahead.length }}</el-tag>
          <span class="summary-text">{{ compare.result.other }} 领先 {{ compare.result.base }}</span>
          <el-tag type="warning">落后 {{ compare.result.behind.length }}</el-tag>
          <span class="summary-text">{{ compare.result.other }} 落后 {{ compare.result.base }}</span>
        </div>
        <el-tabs v-model="compare.tab">
          <el-tab-pane :label="`领先 Commits（${compare.result.ahead.length}）`" name="ahead">
            <div v-for="c in compare.result.ahead" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <el-empty v-if="compare.result.ahead.length === 0" description="无" :image-size="40" />
          </el-tab-pane>
          <el-tab-pane :label="`落后 Commits（${compare.result.behind.length}）`" name="behind">
            <div v-for="c in compare.result.behind" :key="c.oid" class="commit-row">
              <span class="commit-oid">{{ c.shortOid }}</span>
              <span class="commit-msg">{{ c.message }}</span>
              <span class="commit-meta">{{ c.author }} · {{ c.time }}</span>
            </div>
            <el-empty v-if="compare.result.behind.length === 0" description="无" :image-size="40" />
          </el-tab-pane>
          <el-tab-pane :label="`文件差异（${compare.result.files.length}）`" name="files">
            <div class="compare-files">
              <div class="file-list">
                <div
                  v-for="f in compare.result.files"
                  :key="f.newPath"
                  :class="['file-item', { active: compare.selectedFile?.newPath === f.newPath }]"
                  @click="compare.selectedFile = f"
                >
                  <span :class="['file-status-icon', f.status]">{{ statusIcon(f.status) }}</span>
                  <span class="file-name">{{ f.newPath }}</span>
                </div>
                <el-empty v-if="compare.result.files.length === 0" description="无文件差异" :image-size="40" />
              </div>
              <div class="file-diff">
                <UnifiedDiff v-if="compare.selectedFile" :file="compare.selectedFile" />
                <el-empty v-else description="选择文件查看 Diff" :image-size="40" />
              </div>
            </div>
          </el-tab-pane>
        </el-tabs>
      </div>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, MoreFilled, Plus, Refresh, Switch } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import {
  checkoutBranch,
  compareBranches,
  createBranch,
  deleteBranch,
  listBranches,
  pushBranch,
  renameBranch,
  setUpstream,
  trackRemoteBranch,
} from "@/api/branch";
import type { BranchEntry, BranchOverview, CompareResult, RemoteBranchEntry } from "@/types/branch";
import type { FileDiff } from "@/types/git";
import { syncPull } from "@/api/git_ops";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();

const repoPath = ref("");
const overview = ref<BranchOverview | null>(null);
const loading = ref(false);

const compare = reactive<{
  show: boolean;
  base: string;
  other: string;
  loading: boolean;
  result: CompareResult | null;
  tab: "ahead" | "behind" | "files";
  selectedFile: FileDiff | null;
}>({
  show: false,
  base: "",
  other: "",
  loading: false,
  result: null,
  tab: "ahead",
  selectedFile: null,
});

/** Compare revisions: local + remote branch names and tags. */
const revisionOptions = computed<string[]>(() => {
  if (!overview.value) return [];
  return [
    ...overview.value.locals.map((b) => b.name),
    ...overview.value.remotes.map((r) => r.name),
    ...overview.value.tags.map((t) => t.name),
  ];
});

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
    overview.value = await listBranches(repoPath.value);
  } catch (e) {
    ElMessage.error("获取分支列表失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function goBack() {
  router.push({ name: "repository-list" });
}

function shortOid(oid: string): string {
  return oid ? oid.slice(0, 7) : "";
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

// ---------------------------------------------------------------------------
// Local branch commands (§46: Delete = Dangerous 二次确认; Push = Warning 确认)
// ---------------------------------------------------------------------------

async function handleLocalCommand(cmd: string, b: BranchEntry) {
  switch (cmd) {
    case "checkout":
      await runOp(`已切换到分支 ${b.name}`, () => checkoutBranch(repoPath.value, b.name));
      break;
    case "rename":
      await handleRename(b);
      break;
    case "set-upstream":
      await handleSetUpstream(b);
      break;
    case "push":
      await handlePush(b);
      break;
    case "pull":
      // --ff-only pull onto the current branch; divergent state fails safely.
      await runOp("Pull 完成", () => syncPull(repoPath.value));
      break;
    case "compare":
      openCompare(b.name);
      break;
    case "delete":
      await handleDelete(b);
      break;
  }
}

async function handleRemoteCommand(cmd: string, r: RemoteBranchEntry) {
  switch (cmd) {
    case "track":
      await runOp(`已创建跟踪分支（${r.name}）`, () =>
        trackRemoteBranch(repoPath.value, r.name),
      );
      break;
    case "compare":
      openCompare(r.name);
      break;
  }
}

/** Run an op, toast the result, reload on success. */
async function runOp(successMsg: string, op: () => Promise<unknown>) {
  try {
    await op();
    ElMessage.success(successMsg);
    await load();
  } catch (e) {
    ElMessage.error(errMsg(e));
  }
}

async function handleCreate() {
  try {
    const { value: name } = await ElMessageBox.prompt(
      "新分支名称（基于当前 HEAD；可在下方输入框留空目标）",
      "新建分支",
      {
        confirmButtonText: "创建",
        cancelButtonText: "取消",
        inputPattern: /^[^\s~^:?*[\]\\]+$/,
        inputErrorMessage: "分支名不合法",
      },
    );
    if (!name) return;
    await runOp(`已创建分支 ${name}`, () => createBranch(repoPath.value, name));
  } catch (e) {
    if (e !== "cancel") ElMessage.error("创建分支失败: " + errMsg(e));
  }
}

async function handleRename(b: BranchEntry) {
  try {
    const { value: newName } = await ElMessageBox.prompt(
      `将分支 ${b.name} 重命名为：`,
      "Rename Branch",
      {
        confirmButtonText: "重命名",
        cancelButtonText: "取消",
        inputValue: b.name,
        inputPattern: /^[^\s~^:?*[\]\\]+$/,
        inputErrorMessage: "分支名不合法",
      },
    );
    if (!newName || newName === b.name) return;
    await runOp(`已重命名为 ${newName}`, () =>
      renameBranch(repoPath.value, b.name, newName),
    );
  } catch (e) {
    if (e !== "cancel") ElMessage.error("重命名失败: " + errMsg(e));
  }
}

async function handleSetUpstream(b: BranchEntry) {
  if (!overview.value) return;
  const options = overview.value.remotes.map((r) => r.name);
  try {
    const { value } = await ElMessageBox.prompt(
      `设置 ${b.name} 的上游（输入远程分支名，如 origin/main；输入 "-" 清除上游）：`,
      "Set Upstream",
      {
        confirmButtonText: "确定",
        cancelButtonText: "取消",
        inputValue: b.upstream ?? (options.length === 1 ? options[0] : ""),
        inputValidator: (v: string) =>
          v === "-" || options.includes(v) || "必须是现有远程分支名，或 - 清除",
      },
    );
    const upstream = value === "-" ? undefined : value;
    await runOp(
      upstream ? `已设置上游 ${upstream}` : "已清除上游",
      () => setUpstream(repoPath.value, b.name, upstream),
    );
  } catch (e) {
    if (e !== "cancel") ElMessage.error("设置上游失败: " + errMsg(e));
  }
}

async function handlePush(b: BranchEntry) {
  try {
    await ElMessageBox.confirm(
      `推送本地分支 ${b.name} 到 ${b.upstream ?? "默认远程"}？（不启用 force）`,
      "Push 确认",
      { confirmButtonText: "Push", cancelButtonText: "取消", type: "warning" },
    );
  } catch {
    return;
  }
  try {
    const output = await pushBranch(repoPath.value, b.name);
    ElMessage.success(output ? `Push 完成：${output}` : "Push 完成");
    await load();
  } catch (e) {
    ElMessage.error("Push 失败: " + errMsg(e));
  }
}

/** Dangerous op (§46): 二次确认；未合入时升级为强制删除确认。 */
async function handleDelete(b: BranchEntry) {
  try {
    await ElMessageBox.confirm(
      `确认删除本地分支 ${b.name}？此操作不可撤销（可用 reflog 尝试找回）。`,
      "Delete 确认（Dangerous）",
      {
        confirmButtonText: "删除",
        cancelButtonText: "取消",
        type: "error",
        confirmButtonClass: "el-button--danger",
      },
    );
  } catch {
    return;
  }
  try {
    await deleteBranch(repoPath.value, b.name);
    ElMessage.success(`已删除分支 ${b.name}`);
    await load();
  } catch (e) {
    const msg = errMsg(e);
    if (msg.includes("not fully merged")) {
      // Second gate: force-delete confirmation for unmerged branches.
      try {
        await ElMessageBox.confirm(
          `分支 ${b.name} 未完全合入当前 HEAD，删除可能丢失提交。确认强制删除？`,
          "强制删除确认（Dangerous）",
          {
            confirmButtonText: "强制删除",
            cancelButtonText: "取消",
            type: "error",
            confirmButtonClass: "el-button--danger",
          },
        );
      } catch {
        return;
      }
      await runOp(`已强制删除分支 ${b.name}`, () =>
        deleteBranch(repoPath.value, b.name, true),
      );
    } else {
      ElMessage.error("删除失败: " + msg);
    }
  }
}

// ---------------------------------------------------------------------------
// Compare
// ---------------------------------------------------------------------------

function openCompare(presetOther?: string) {
  compare.base = overview.value?.current ?? overview.value?.locals[0]?.name ?? "";
  compare.other = presetOther ?? "";
  compare.result = null;
  compare.selectedFile = null;
  compare.tab = "ahead";
  compare.show = true;
}

async function runCompare() {
  compare.loading = true;
  compare.selectedFile = null;
  try {
    compare.result = await compareBranches(repoPath.value, compare.base, compare.other);
    compare.tab = "ahead";
  } catch (e) {
    ElMessage.error("Compare 失败: " + errMsg(e));
  } finally {
    compare.loading = false;
  }
}
</script>

<style scoped>
.branch-manager {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.branch-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 16px;
  border-bottom: 1px solid #ebeef5;
  background: #fff;
}

.repo-info {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.repo-path {
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-body {
  flex: 1;
  overflow-y: auto;
  padding: 12px 16px;
  background: #fafafa;
}

.section {
  background: #fff;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  margin-bottom: 12px;
}

.section-title {
  padding: 8px 12px;
  font-size: 13px;
  font-weight: 600;
  color: #606266;
  border-bottom: 1px solid #f0f0f0;
}

.branch-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 12px;
  border-bottom: 1px solid #f5f5f5;
  font-size: 13px;
}

.branch-row:last-child {
  border-bottom: none;
}

.branch-row.current {
  background: #f0f9eb;
}

.branch-name {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 220px;
  font-weight: 500;
  flex-shrink: 0;
}

.branch-track {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 200px;
  flex-shrink: 0;
}

.upstream {
  color: #909399;
}

.ahead {
  color: #67c23a;
  font-weight: 600;
}

.behind {
  color: #e6a23c;
  font-weight: 600;
}

.no-upstream {
  color: #c0c4cc;
  font-size: 12px;
}

.tag-message {
  color: #909399;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.branch-commit {
  flex: 1;
  color: #606266;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.danger-item {
  color: #f56c6c;
}

.compare-form {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}

.compare-arrow {
  color: #909399;
}

.compare-summary {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.summary-text {
  color: #606266;
  font-size: 13px;
  margin-right: 8px;
}

.commit-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 4px 8px;
  font-size: 13px;
  border-bottom: 1px solid #f5f5f5;
}

.commit-oid {
  font-family: "Cascadia Code", Consolas, monospace;
  color: #409eff;
  width: 70px;
  flex-shrink: 0;
}

.commit-msg {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-meta {
  color: #909399;
  font-size: 12px;
  flex-shrink: 0;
}

.compare-files {
  display: flex;
  height: 50vh;
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
