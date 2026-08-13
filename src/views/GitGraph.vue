<template>
  <div class="git-graph-view">
    <!-- Header -->
    <div class="graph-header">
      <el-button @click="goBack">
        <el-icon><ArrowLeft /></el-icon>
        返回
      </el-button>
      <span class="repo-path">{{ repoPath }}</span>
      <el-button
        type="primary"
        size="small"
        :loading="loading"
        @click="loadHistory"
      >
        刷新
      </el-button>
    </div>

    <!-- Branch bar -->
    <div v-if="branches.length > 0" class="branch-bar">
      <el-tag
        v-for="branch in branches.slice(0, 10)"
        :key="branch.name"
        :type="branch.isCurrent ? 'success' : branch.isRemote ? 'warning' : 'info'"
        size="small"
        effect="plain"
      >
        {{ branch.name }}
      </el-tag>
    </div>

    <!-- Commit graph -->
    <div class="graph-body" v-loading="loading">
      <CommitGraph
        :commits="commits"
        :loading="loading"
        :has-more="hasMore"
        @select="onCommitSelect"
        @load-more="loadMore"
      />
    </div>

    <!-- Commit detail -->
    <el-drawer
      v-model="showDetail"
      title="提交详情"
      direction="rtl"
      size="400px"
    >
      <div v-if="selectedCommit" class="commit-detail">
        <el-descriptions :column="1" border>
          <el-descriptions-item label="Hash">
            {{ selectedCommit.oid }}
          </el-descriptions-item>
          <el-descriptions-item label="作者">
            {{ selectedCommit.author }}
            &lt;{{ selectedCommit.email }}&gt;
          </el-descriptions-item>
          <el-descriptions-item label="时间">
            {{ selectedCommit.time }}
          </el-descriptions-item>
          <el-descriptions-item label="Refs">
            <el-tag
              v-for="ref in selectedCommit.refs"
              :key="ref"
              size="small"
              style="margin-right: 4px"
            >
              {{ ref }}
            </el-tag>
          </el-descriptions-item>
          <el-descriptions-item label="提交信息">
            <pre class="commit-message-full">{{ selectedCommit.message }}</pre>
          </el-descriptions-item>
        </el-descriptions>
      </div>
    </el-drawer>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { ArrowLeft } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import { getCommitHistory, getBranches } from "@/api/graph";
import type { CommitInfo, BranchInfo } from "@/types/graph";
import CommitGraph from "@/components/graph/CommitGraph.vue";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();

const repoPath = ref("");
const commits = ref<CommitInfo[]>([]);
const branches = ref<BranchInfo[]>([]);
const loading = ref(false);
const hasMore = ref(false);
const showDetail = ref(false);
const selectedCommit = ref<CommitInfo | null>(null);

const PAGE_SIZE = 100;

onMounted(async () => {
  const repo = route.query.repo as string;
  if (!repo) {
    ElMessage.warning("未指定仓库路径");
    router.push({ name: "repository-list" });
    return;
  }
  repoPath.value = repo;
  await loadHistory();
  await loadBranches();
});

async function loadHistory() {
  loading.value = true;
  try {
    commits.value = await getCommitHistory(repoPath.value, PAGE_SIZE);
    hasMore.value = commits.value.length >= PAGE_SIZE;
  } catch (e) {
    ElMessage.error("加载提交历史失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function loadBranches() {
  try {
    branches.value = await getBranches(repoPath.value);
  } catch (e) {
    console.error("Failed to load branches:", e);
  }
}

async function loadMore() {
  loading.value = true;
  try {
    const more = await getCommitHistory(
      repoPath.value,
      commits.value.length + PAGE_SIZE,
    );
    if (more.length > commits.value.length) {
      commits.value = more;
      hasMore.value = more.length >= commits.value.length + PAGE_SIZE;
    } else {
      hasMore.value = false;
    }
  } catch (e) {
    ElMessage.error("加载更多失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function onCommitSelect(commit: CommitInfo) {
  selectedCommit.value = commit;
  showDetail.value = true;
}

function goBack() {
  router.push({ name: "repository-list" });
}
</script>

<style scoped>
.git-graph-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.graph-header {
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

.branch-bar {
  display: flex;
  gap: 4px;
  padding: 4px 16px;
  border-bottom: 1px solid #ebeef5;
  background: #fafafa;
  flex-wrap: wrap;
}

.graph-body {
  flex: 1;
  overflow: hidden;
}

.commit-detail {
  padding: 12px;
}

.commit-message-full {
  white-space: pre-wrap;
  word-break: break-word;
  font-family: inherit;
  font-size: 13px;
  margin: 0;
}
</style>
