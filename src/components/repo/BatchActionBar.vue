<template>
  <div v-if="selectedCount > 0" class="batch-action-bar">
    <span class="selection-info">
      已选 {{ selectedCount }} 个仓库
    </span>
    <n-space>
      <n-button size="small" :loading="loading" @click="handleAction('fetch')">
        <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
        Fetch
      </n-button>
      <n-button size="small" :loading="loading" @click="handleAction('pull')">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        Pull
      </n-button>
      <n-button size="small" :loading="loading" @click="handleAction('push')">
        <template #icon><n-icon><CloudUploadOutline /></n-icon></template>
        Push
      </n-button>
    </n-space>
    <n-button size="small" :loading="loading" @click="showCommitDialog = true">
      <template #icon><n-icon><CreateOutline /></n-icon></template>
      Commit
    </n-button>

    <!-- Commit Dialog -->
    <n-modal :show="showCommitDialog" preset="card" title="批量提交" style="width: 500px" @update:show="(v: boolean) => showCommitDialog = v">
      <n-form :model="commitForm" label-width="80px">
        <n-form-item label="提交信息">
          <n-input
            v-model:value="commitForm.message"
            type="textarea"
            :rows="3"
            placeholder="请输入 commit message"
          />
        </n-form-item>
        <n-form-item label="文件路径">
          <n-input
            v-model:value="commitForm.filesInput"
            type="textarea"
            :rows="4"
            placeholder="每行一个文件路径（留空则提交所有更改）"
          />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="showCommitDialog = false">取消</n-button>
        <n-button type="primary" @click="handleCommit">提交</n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useMessage } from "naive-ui";
import { CloudUploadOutline, RefreshOutline, CreateOutline } from "@vicons/ionicons5";
import * as gitOpsApi from "@/api/git_ops";
import type { CommitRequest } from "@/types/task";
import { errMsg } from "@/utils/error";

const message = useMessage();

const props = defineProps<{
  selectedPaths: string[];
}>();

const emit = defineEmits<{
  (e: "action-completed"): void;
}>();

const loading = ref(false);
const showCommitDialog = ref(false);
const commitForm = ref({
  message: "",
  filesInput: "",
});

const selectedCount = props.selectedPaths.length;

async function handleAction(action: "fetch" | "pull" | "push") {
  if (props.selectedPaths.length === 0) return;
  loading.value = true;
  try {
    const paths = props.selectedPaths;
    let taskIds: string[];
    if (action === "fetch") {
      taskIds = await gitOpsApi.batchFetch(paths);
    } else if (action === "pull") {
      taskIds = await gitOpsApi.batchPull(paths);
    } else {
      taskIds = await gitOpsApi.batchPush(paths);
    }
    message.success(`已提交 ${taskIds.length} 个${action}任务`);
    emit("action-completed");
  } catch (e) {
    message.error(`操作失败: ${errMsg(e)}`);
  } finally {
    loading.value = false;
  }
}

async function handleCommit() {
  if (props.selectedPaths.length === 0) return;
  if (!commitForm.value.message.trim()) {
    message.warning("请输入提交信息");
    return;
  }

  loading.value = true;
  try {
    const files = commitForm.value.filesInput
      .split("\n")
      .map((f) => f.trim())
      .filter((f) => f.length > 0);

    const commits: CommitRequest[] = props.selectedPaths.map((path) => {
      const name = path.split(/[\\/]/).pop() || "unknown";
      return {
        repoPath: path,
        repoName: name,
        message: commitForm.value.message,
        files,
      };
    });

    const taskIds = await gitOpsApi.batchCommit(commits);
    message.success(`已提交 ${taskIds.length} 个 commit 任务`);
    showCommitDialog.value = false;
    commitForm.value.message = "";
    commitForm.value.filesInput = "";
    emit("action-completed");
  } catch (e) {
    message.error(`提交失败: ${errMsg(e)}`);
  } finally {
    loading.value = false;
  }
}
</script>

<style scoped>
.batch-action-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: #ecf5ff;
  border-radius: 4px;
  margin-bottom: 8px;
}

.selection-info {
  font-size: 13px;
  color: #409eff;
  font-weight: 500;
}
</style>
