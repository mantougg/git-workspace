<template>
  <div class="reflog-view">
    <!-- Header -->
    <div class="reflog-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <span class="repo-path">{{ repoPath }}</span>
      <el-select
        v-model="reference"
        style="width: 220px"
        size="small"
        @change="load"
      >
        <el-option label="HEAD" value="HEAD" />
        <el-option-group v-if="locals.length > 0" label="Local Branches">
          <el-option v-for="b in locals" :key="b" :label="b" :value="b" />
        </el-option-group>
        <el-option-group v-if="remotes.length > 0" label="Remote Branches">
          <el-option v-for="r in remotes" :key="r" :label="r" :value="r" />
        </el-option-group>
      </el-select>
      <el-button size="small" :loading="loading" @click="load">
        <el-icon><Refresh /></el-icon>
        刷新
      </el-button>
    </div>

    <!-- Entry list -->
    <div class="reflog-body" v-loading="loading">
      <div v-for="entry in entries" :key="entry.selector" class="reflog-row">
        <span class="selector">{{ entry.selector }}</span>
        <span class="summary" :title="entry.summary">{{ entry.summary }}</span>
        <span class="commit-message" :title="entry.newOid">
          {{ entry.newOid.slice(0, 7) }} {{ entry.commitMessage }}
        </span>
        <span class="time">{{ entry.time }}</span>
        <el-dropdown trigger="click" @command="(cmd: string) => onAction(cmd, entry)">
          <el-button size="small" text @click.stop>
            <el-icon><MoreFilled /></el-icon>
          </el-button>
          <template #dropdown>
            <el-dropdown-menu>
              <el-dropdown-item command="view">View Commit</el-dropdown-item>
              <el-dropdown-item command="branch">Create Branch Here</el-dropdown-item>
              <el-dropdown-item command="reset" divided>Reset Here…</el-dropdown-item>
              <el-dropdown-item command="restore">
                <span class="danger-item">Restore State（hard）</span>
              </el-dropdown-item>
            </el-dropdown-menu>
          </template>
        </el-dropdown>
      </div>
      <el-empty v-if="!loading && entries.length === 0" description="暂无 reflog 记录" :image-size="60" />
    </div>

    <!-- View Commit dialog -->
    <el-dialog v-model="viewDialog.show" title="提交详情" width="520px">
      <el-descriptions v-if="viewDialog.entry" :column="1" border>
        <el-descriptions-item label="位置">{{ viewDialog.entry.selector }}</el-descriptions-item>
        <el-descriptions-item label="Hash">{{ viewDialog.entry.newOid }}</el-descriptions-item>
        <el-descriptions-item label="提交信息">{{ viewDialog.entry.commitMessage }}</el-descriptions-item>
        <el-descriptions-item label="时间">{{ viewDialog.entry.time }}</el-descriptions-item>
        <el-descriptions-item label="Reflog 动作">{{ viewDialog.entry.summary }}</el-descriptions-item>
      </el-descriptions>
    </el-dialog>

    <!-- Reset Here dialog -->
    <el-dialog v-model="resetDialog.show" title="Reset Here" width="520px">
      <div v-if="resetDialog.entry" class="reset-target">
        目标：{{ resetDialog.entry.newOid.slice(0, 7) }} {{ resetDialog.entry.commitMessage }}
        （{{ resetDialog.entry.selector }}）
      </div>
      <el-radio-group v-model="resetDialog.mode" class="reset-modes">
        <el-radio value="soft">soft — 仅移动 HEAD，保留暂存区与工作区</el-radio>
        <el-radio value="mixed">mixed — 移动 HEAD + 重置暂存区，保留工作区</el-radio>
        <el-radio value="hard">hard — 重置全部，丢弃未提交更改（危险）</el-radio>
      </el-radio-group>
      <template #footer>
        <el-button @click="resetDialog.show = false">取消</el-button>
        <el-button
          :type="resetDialog.mode === 'hard' ? 'danger' : 'primary'"
          @click="confirmReset"
        >
          执行 Reset
        </el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, MoreFilled, Refresh } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { getReflog } from "@/api/reflog";
import { listBranches, createBranch } from "@/api/branch";
import { resetTo } from "@/api/history";
import type { ReflogEntry } from "@/types/reflog";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();

const repoPath = ref("");
const reference = ref("HEAD");
const locals = ref<string[]>([]);
const remotes = ref<string[]>([]);
const entries = ref<ReflogEntry[]>([]);
const loading = ref(false);

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
  const repo = route.query.repo as string;
  if (!repo) {
    ElMessage.warning("未指定仓库路径");
    router.push({ name: "changes" });
    return;
  }
  repoPath.value = repo;
  try {
    const overview = await listBranches(repoPath.value);
    locals.value = overview.locals.map((b) => b.name);
    remotes.value = overview.remotes.map((r) => r.name);
  } catch {
    // 分支列表只用于选择器选项，失败不阻塞 reflog 展示
  }
  await load();
});

async function load() {
  loading.value = true;
  try {
    entries.value = await getReflog(repoPath.value, reference.value, 200);
  } catch (e) {
    entries.value = [];
    ElMessage.error("读取 reflog 失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function goBack() {
  router.push({ name: "git-graph", query: { repo: repoPath.value } });
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
    const { value: name } = await ElMessageBox.prompt(
      `在 ${entry.selector}（${entry.newOid.slice(0, 7)} ${entry.commitMessage}）处创建分支：`,
      "Create Branch Here",
      {
        confirmButtonText: "创建",
        cancelButtonText: "取消",
        inputPattern: /^[^\s~^:?*[\]\\]+$/,
        inputErrorMessage: "分支名不合法",
      },
    );
    if (!name) return;
    await createBranch(repoPath.value, name, entry.newOid);
    ElMessage.success(`已创建分支 ${name}`);
  } catch (e) {
    if (e !== "cancel") ElMessage.error("创建分支失败: " + errMsg(e));
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
      await ElMessageBox.confirm(dangerConfirmText(entry), "Reset --hard 确认（Dangerous）", {
        confirmButtonText: "确认 Hard Reset",
        cancelButtonText: "取消",
        type: "error",
        confirmButtonClass: "el-button--danger",
      });
    } catch {
      return;
    }
  }

  resetDialog.show = false;
  try {
    await resetTo(repoPath.value, entry.newOid, mode);
    ElMessage.success(`已 Reset 到 ${entry.selector}（${mode}）`);
    await load();
  } catch (e) {
    ElMessage.error("Reset 失败: " + errMsg(e));
  }
}

/** Restore State = hard reset shortcut with Dangerous confirm (§46). */
async function handleRestore(entry: ReflogEntry) {
  try {
    await ElMessageBox.confirm(dangerConfirmText(entry), "Restore State 确认（Dangerous）", {
      confirmButtonText: "恢复到此状态",
      cancelButtonText: "取消",
      type: "error",
      confirmButtonClass: "el-button--danger",
    });
  } catch {
    return;
  }
  try {
    await resetTo(repoPath.value, entry.newOid, "hard");
    ElMessage.success(`已恢复到 ${entry.selector}`);
    await load();
  } catch (e) {
    ElMessage.error("恢复失败: " + errMsg(e));
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

.reflog-body {
  flex: 1;
  overflow-y: auto;
  background: #fff;
}

.reflog-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 6px 16px;
  border-bottom: 1px solid #f5f5f5;
  font-size: 13px;
}

.selector {
  width: 140px;
  flex-shrink: 0;
  font-family: "Cascadia Code", Consolas, monospace;
  color: #409eff;
}

.summary {
  width: 260px;
  flex-shrink: 0;
  color: #606266;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.commit-message {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  color: #303133;
}

.time {
  color: #909399;
  font-size: 12px;
  flex-shrink: 0;
}

.danger-item {
  color: #f56c6c;
}

.reset-target {
  margin-bottom: 12px;
  font-size: 13px;
  color: #606266;
}

.reset-modes {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
</style>
