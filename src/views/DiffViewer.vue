<template>
  <div class="diff-viewer">
    <!-- Header -->
    <div class="diff-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <div class="repo-info">
        <span class="repo-path">{{ repoPath }}</span>
        <el-tag v-if="files.length > 0" size="small">
          {{ files.length }} 个文件变更
        </el-tag>
      </div>
      <div class="diff-options">
        <el-checkbox v-model="diffOptions.ignoreWhitespace">
          Ignore Whitespace
        </el-checkbox>
        <el-checkbox v-model="diffOptions.ignoreWhitespaceEol">
          Ignore EOL
        </el-checkbox>
        <el-checkbox v-model="diffOptions.ignoreCase">
          Ignore Case
        </el-checkbox>
      </div>
      <el-radio-group v-model="diffMode" size="small">
        <el-radio-button value="unified">Unified</el-radio-button>
        <el-radio-button value="side-by-side">Side by Side</el-radio-button>
      </el-radio-group>
      <el-button
        v-if="files.length > 0"
        type="primary"
        size="small"
        :loading="reviewLoading"
        @click="handleAiReview"
      >
        <el-icon><MagicStick /></el-icon>
        AI Review
      </el-button>
    </div>

    <!-- AI Review result dialog -->
    <el-dialog
      v-model="showReview"
      title="AI Code Review"
      width="600px"
      :close-on-click-modal="false"
    >
      <div v-if="reviewResult" class="review-result">
        <div class="review-summary">
          <strong>Summary:</strong> {{ reviewResult.summary }}
        </div>
        <div v-if="reviewResult.issues.length > 0" class="review-issues">
          <div
            v-for="(issue, i) in reviewResult.issues"
            :key="i"
            class="review-issue"
          >
            <el-tag
              :type="issue.severity === 'high' ? 'danger' : issue.severity === 'medium' ? 'warning' : 'info'"
              size="small"
            >
              {{ issue.severity }}
            </el-tag>
            <el-tag size="small" effect="plain">
              {{ issue.category }}
            </el-tag>
            <span class="issue-file">{{ issue.file }}</span>
            <div class="issue-desc">{{ issue.description }}</div>
          </div>
        </div>
        <el-empty v-else description="No issues found" :image-size="60" />
      </div>
    </el-dialog>

    <!-- Body -->
    <div class="diff-body" v-loading="loading">
      <template v-if="files.length > 0">
        <!-- File list -->
        <div class="file-list">
          <div
            v-for="file in files"
            :key="file.newPath"
            :class="['file-item', { active: selectedFile?.newPath === file.newPath }]"
            @click="selectedFile = file"
          >
            <span :class="['file-status-icon', file.status]">
              {{ statusIcon(file.status) }}
            </span>
            <span class="file-name">{{ file.newPath }}</span>
          </div>
        </div>

        <!-- Diff content -->
        <div class="diff-content">
          <div v-if="selectedFile" class="file-diff">
            <div class="file-diff-header">
              {{ selectedFile.newPath }}
            </div>
            <div class="file-diff-body">
              <UnifiedDiff
                v-if="diffMode === 'unified'"
                :file="selectedFile"
              />
              <SideBySideDiff v-else :file="selectedFile" />
            </div>
          </div>
          <el-empty v-else description="选择文件查看 Diff" />
        </div>
      </template>
      <el-empty v-else-if="!loading" description="没有文件变更，工作区是干净的" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft, MagicStick } from "@element-plus/icons-vue";
import { ElMessage, ElMessageBox } from "element-plus";
import { getDiff, type DiffOptions } from "@/api/git";
import { aiReview } from "@/api/ai";
import type { ReviewResult } from "@/types/ai";
import type { FileDiff } from "@/types/git";
import UnifiedDiff from "@/components/diff/UnifiedDiff.vue";
import SideBySideDiff from "@/components/diff/SideBySideDiff.vue";
import { errMsg } from "@/utils/error";
import { startFrameMeter, type FrameStats } from "@/utils/frameTime";

const route = useRoute();
const router = useRouter();

const repoPath = ref("");
const files = ref<FileDiff[]>([]);
const selectedFile = ref<FileDiff | null>(null);
const loading = ref(false);
const diffMode = ref<"unified" | "side-by-side">("unified");
const diffOptions = ref<DiffOptions>({
  ignoreWhitespace: false,
  ignoreWhitespaceEol: false,
  ignoreCase: false,
});
const reviewLoading = ref(false);
const showReview = ref(false);
const reviewResult = ref<ReviewResult | null>(null);

onMounted(async () => {
  stopFrameMeter = startFrameMeter("diff-viewer");
  const repo = route.query.repo as string;
  if (!repo) {
    ElMessage.warning("未指定仓库路径");
    router.push({ name: "repository-list" });
    return;
  }
  repoPath.value = repo;
  await loadDiff();
});

let stopFrameMeter: (() => FrameStats) | null = null;

// Frame-time measurement channel for the diff rendering budget (T-04):
// samples rAF frame durations while this view is open; live stats are on
// window.__gwPerf["diff-viewer"], slow frames are warned in the console.
onUnmounted(() => {
  const stats = stopFrameMeter?.();
  if (stats && stats.frames > 0) {
    console.debug(
      `[perf:diff-viewer] frames=${stats.frames} avg=${stats.avgMs}ms p95=${stats.p95Ms}ms max=${stats.maxMs}ms`,
    );
  }
});

let loadSeq = 0;

async function loadDiff() {
  const seq = ++loadSeq;
  loading.value = true;
  try {
    const next = await getDiff(repoPath.value, diffOptions.value);
    if (seq !== loadSeq) return; // 已有更新的请求，丢弃过期结果
    files.value = next;
    // 保留当前选中文件，避免切换设置时跳回第一个文件
    const currentPath = selectedFile.value?.newPath;
    selectedFile.value =
      next.find((f) => f.newPath === currentPath) ?? next[0] ?? null;
  } catch (e) {
    if (seq !== loadSeq) return;
    ElMessage.error("获取 Diff 失败: " + errMsg(e));
  } finally {
    if (seq === loadSeq) loading.value = false;
  }
}

// 切换 Ignore 设置时即时重新计算 diff（Roadmap §9）
watch(diffOptions, () => loadDiff(), { deep: true });

function goBack() {
  router.push({ name: "repository-list" });
}

async function handleAiReview() {
  try {
    const { value: apiKey } = await ElMessageBox.prompt(
      "请输入您的 AI API Key",
      "AI Code Review",
      {
        confirmButtonText: "开始审查",
        cancelButtonText: "取消",
        inputType: "password",
        inputPlaceholder: "OpenAI API Key",
      },
    );

    if (!apiKey) return;

    reviewLoading.value = true;
    reviewResult.value = await aiReview(repoPath.value, apiKey);
    showReview.value = true;
  } catch (e) {
    if (e !== "cancel") {
      ElMessage.error("AI Review 失败: " + errMsg(e));
    }
  } finally {
    reviewLoading.value = false;
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
</script>

<style scoped>
.diff-viewer {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.diff-header {
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
}

.diff-options {
  display: flex;
  align-items: center;
  gap: 12px;
  white-space: nowrap;
}

.repo-path {
  font-size: 14px;
  font-weight: 500;
}

.diff-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.file-list {
  width: 280px;
  border-right: 1px solid #ebeef5;
  overflow-y: auto;
  background: #fafafa;
}

.file-item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  cursor: pointer;
  border-bottom: 1px solid #f0f0f0;
  font-size: 13px;
}

.file-item:hover {
  background: #f5f7fa;
}

.file-item.active {
  background: #ecf5ff;
  border-left: 3px solid #409eff;
  padding-left: 9px;
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
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.diff-content {
  flex: 1;
  overflow: auto;
  background: #fff;
}

.file-diff {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.file-diff-header {
  padding: 6px 12px;
  background: #f5f7fa;
  border-bottom: 1px solid #ebeef5;
  font-size: 13px;
  font-weight: 500;
}

.file-diff-body {
  flex: 1;
  overflow: auto;
}

.review-result {
  padding: 12px;
}

.review-summary {
  margin-bottom: 12px;
  font-size: 14px;
  line-height: 1.6;
}

.review-issues {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.review-issue {
  padding: 8px;
  border: 1px solid #ebeef5;
  border-radius: 4px;
  display: flex;
  align-items: flex-start;
  gap: 6px;
  flex-wrap: wrap;
}

.issue-file {
  font-family: monospace;
  font-size: 12px;
  color: #606266;
}

.issue-desc {
  width: 100%;
  font-size: 13px;
  color: #303133;
  margin-top: 4px;
}
</style>

/* VirtualList owns diff-body scrolling now (T-04); clip the outer container
   so a double scrollbar can never appear. */
.file-diff-body {
  overflow: hidden;
}
