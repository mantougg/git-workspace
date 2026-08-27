<template>
  <div class="reflog-view">
    <!-- Header -->
    <div class="reflog-header">
      <span class="repo-path">{{ repoPath }}</span>
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
import { useRoute, useRouter } from "vue-router";
import { EllipsisVerticalOutline, RefreshOutline } from "@vicons/ionicons5";
import { useMessage, useDialog } from "naive-ui";
import { prompt } from "@/utils/prompt";
import { getReflog } from "@/api/reflog";
import { listBranches, createBranch } from "@/api/branch";
import { resetTo } from "@/api/history";
import type { ReflogEntry } from "@/types/reflog";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();
const message = useMessage();
const dialog = useDialog();

const repoPath = ref("");
const reference = ref("HEAD");
const locals = ref<string[]>([]);
const remotes = ref<string[]>([]);
const entries = ref<ReflogEntry[]>([]);
const loading = ref(false);

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
  const repo = route.query.repo as string;
  if (!repo) {
    message.warning("未指定仓库路径");
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
    message.error("读取 reflog 失败: " + errMsg(e));
  } finally {
    loading.value = false;
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
