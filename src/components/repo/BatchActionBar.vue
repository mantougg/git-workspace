<template>
  <div v-if="selectedCount > 0" class="batch-action-bar">
    <span class="selection-info">
      已选 {{ selectedCount }} 个仓库
    </span>
    <el-button-group>
      <el-button size="small" :loading="loading" @click="handleAction('fetch')">
        <el-icon><Download /></el-icon>
        Fetch
      </el-button>
      <el-button size="small" :loading="loading" @click="handleAction('pull')">
        <el-icon><Refresh /></el-icon>
        Pull
      </el-button>
      <el-button size="small" :loading="loading" @click="handleAction('push')">
        <el-icon><Upload /></el-icon>
        Push
      </el-button>
    </el-button-group>
    <el-button size="small" :loading="loading" @click="showCommitDialog = true">
      <el-icon><EditPen /></el-icon>
      Commit
    </el-button>

    <!-- Commit Dialog -->
    <el-dialog v-model="showCommitDialog" title="批量提交" width="500px">
      <el-form :model="commitForm" label-width="80px">
        <el-form-item label="提交信息">
          <el-input
            v-model="commitForm.message"
            type="textarea"
            :rows="3"
            placeholder="请输入 commit message"
          />
        </el-form-item>
        <el-form-item label="文件路径">
          <el-input
            v-model="commitForm.filesInput"
            type="textarea"
            :rows="4"
            placeholder="每行一个文件路径（留空则提交所有更改）"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showCommitDialog = false">取消</el-button>
        <el-button type="primary" @click="handleCommit">提交</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { Download, Refresh, Upload, EditPen } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import * as gitOpsApi from "@/api/git_ops";
import type { CommitRequest } from "@/types/task";
import { errMsg } from "@/utils/error";

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
    ElMessage.success(`已提交 ${taskIds.length} 个${action}任务`);
    emit("action-completed");
  } catch (e) {
    ElMessage.error(`操作失败: ${errMsg(e)}`);
  } finally {
    loading.value = false;
  }
}

async function handleCommit() {
  if (props.selectedPaths.length === 0) return;
  if (!commitForm.value.message.trim()) {
    ElMessage.warning("请输入提交信息");
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
    ElMessage.success(`已提交 ${taskIds.length} 个 commit 任务`);
    showCommitDialog.value = false;
    commitForm.value.message = "";
    commitForm.value.filesInput = "";
    emit("action-completed");
  } catch (e) {
    ElMessage.error(`提交失败: ${errMsg(e)}`);
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
