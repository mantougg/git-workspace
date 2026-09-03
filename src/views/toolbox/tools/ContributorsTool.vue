<template>
  <div class="contributors-tool">
    <div class="section">
      <div class="section-title">项目信息</div>
      <div class="input-row">
        <n-input
          v-model:value="repoUrl"
          placeholder="输入 GitHub 仓库 URL 或 owner/repo..."
        />
        <n-button type="primary" @click="fetchContributors" :loading="loading">
          查询
        </n-button>
      </div>
    </div>

    <div v-if="error" class="error-section">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="repoInfo" class="section">
      <div class="section-title">仓库信息</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="名称">{{ repoInfo.full_name }}</n-descriptions-item>
        <n-descriptions-item label="Stars">{{ repoInfo.stargazers_count }}</n-descriptions-item>
        <n-descriptions-item label="Forks">{{ repoInfo.forks_count }}</n-descriptions-item>
        <n-descriptions-item label="Issues">{{ repoInfo.open_issues_count }}</n-descriptions-item>
        <n-descriptions-item label="语言">{{ repoInfo.language || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="大小">{{ formatSize(repoInfo.size) }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div v-if="contributors.length > 0" class="section">
      <div class="section-title">贡献者列表 ({{ contributors.length }})</div>
      <div class="contributors-list">
        <div
          v-for="contributor in contributors"
          :key="contributor.id"
          class="contributor-item"
        >
          <div class="contributor-avatar">
            <img :src="contributor.avatar_url" :alt="contributor.login" />
          </div>
          <div class="contributor-info">
            <div class="contributor-name">
              <a :href="contributor.html_url" target="_blank">
                {{ contributor.login }}
              </a>
            </div>
            <div class="contributor-commits">
              {{ contributor.contributions }} 次提交
            </div>
          </div>
          <div class="contributor-bar">
            <div
              class="bar-fill"
              :style="{ width: getBarWidth(contributor.contributions) + '%' }"
            />
          </div>
        </div>
      </div>
    </div>

    <div v-if="contributors.length > 0" class="section">
      <div class="section-title">统计图表</div>
      <div class="chart-container">
        <div class="pie-chart">
          <div
            v-for="(item, index) in topContributors"
            :key="item.id"
            class="pie-segment"
            :style="{
              '--start': item.startAngle + 'deg',
              '--end': item.endAngle + 'deg',
              '--color': colors[index % colors.length],
            }"
          />
        </div>
        <div class="legend">
          <div
            v-for="(item, index) in topContributors"
            :key="item.id"
            class="legend-item"
          >
            <span
              class="legend-color"
              :style="{ background: colors[index % colors.length] }"
            />
            <span class="legend-name">{{ item.login }}</span>
            <span class="legend-value">{{ item.contributions }}</span>
          </div>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">常用仓库</div>
      <div class="popular-repos">
        <div
          v-for="repo in popularRepos"
          :key="repo"
          class="popular-repo"
          @click="selectRepo(repo)"
        >
          {{ repo }}
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const repoUrl = ref('')
const loading = ref(false)
const error = ref('')
const repoInfo = ref<any>(null)
const contributors = ref<any[]>([])

const colors = [
  '#1890ff', '#52c41a', '#faad14', '#f5222d', '#722ed1',
  '#13c2c2', '#eb2f96', '#fa8c16', '#2f54eb', '#a0d911',
]

const popularRepos = [
  'vuejs/core',
  'facebook/react',
  'microsoft/vscode',
  'torvalds/linux',
  'rust-lang/rust',
  'golang/go',
  'python/cpython',
  'nodejs/node',
]

const maxContributions = computed(() => {
  if (contributors.value.length === 0) return 0
  return Math.max(...contributors.value.map(c => c.contributions))
})

const topContributors = computed(() => {
  const top = contributors.value.slice(0, 10)
  const total = top.reduce((sum, c) => sum + c.contributions, 0)
  let currentAngle = 0

  return top.map(c => {
    const angle = (c.contributions / total) * 360
    const startAngle = currentAngle
    currentAngle += angle
    return {
      ...c,
      startAngle,
      endAngle: currentAngle,
    }
  })
})

function parseRepoUrl(url: string): string {
  // 处理各种格式
  url = url.trim()
  if (url.includes('github.com')) {
    const match = url.match(/github\.com\/([^/]+\/[^/]+)/)
    if (match) return match[1]
  }
  if (url.includes('/')) return url
  return ''
}

async function fetchContributors() {
  const repo = parseRepoUrl(repoUrl.value)
  if (!repo) {
    error.value = '请输入有效的 GitHub 仓库 URL 或 owner/repo 格式'
    return
  }

  loading.value = true
  error.value = ''
  repoInfo.value = null
  contributors.value = []

  try {
    // 获取仓库信息
    const repoResponse = await fetch(`https://api.github.com/repos/${repo}`)
    if (!repoResponse.ok) {
      throw new Error('仓库不存在或 API 请求失败')
    }
    repoInfo.value = await repoResponse.json()

    // 获取贡献者
    const contributorsResponse = await fetch(
      `https://api.github.com/repos/${repo}/contributors?per_page=30`
    )
    if (!contributorsResponse.ok) {
      throw new Error('获取贡献者失败')
    }
    contributors.value = await contributorsResponse.json()
  } catch (e: any) {
    error.value = e.message
  } finally {
    loading.value = false
  }
}

function formatSize(size: number): string {
  if (size < 1024) return `${size} KB`
  return `${(size / 1024).toFixed(1)} MB`
}

function getBarWidth(contributions: number): number {
  if (maxContributions.value === 0) return 0
  return (contributions / maxContributions.value) * 100
}

function selectRepo(repo: string) {
  repoUrl.value = repo
  fetchContributors()
}
</script>

<style scoped lang="scss">
.contributors-tool {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.section {
  margin-bottom: 16px;
}

.section-title {
  font-weight: 500;
  margin-bottom: 8px;
  color: var(--n-text-color);
}

.input-row {
  display: flex;
  gap: 8px;
}

.error-section {
  margin-top: 8px;
}

.contributors-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.contributor-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.contributor-avatar img {
  width: 40px;
  height: 40px;
  border-radius: 50%;
}

.contributor-info {
  flex: 1;
  min-width: 120px;
}

.contributor-name a {
  color: var(--n-primary-color);
  text-decoration: none;
  font-weight: 500;

  &:hover {
    text-decoration: underline;
  }
}

.contributor-commits {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.contributor-bar {
  flex: 2;
  height: 8px;
  background: var(--n-color);
  border-radius: 4px;
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  background: var(--n-primary-color);
  border-radius: 4px;
  transition: width 0.3s ease;
}

.chart-container {
  display: flex;
  gap: 24px;
  align-items: center;
  flex-wrap: wrap;
}

.pie-chart {
  width: 200px;
  height: 200px;
  border-radius: 50%;
  position: relative;
  background: conic-gradient(
    var(--color) var(--start),
    var(--color) var(--end),
    transparent var(--end)
  );
}

.pie-segment {
  position: absolute;
  inset: 0;
}

.legend {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.legend-item {
  display: flex;
  align-items: center;
  gap: 8px;
}

.legend-color {
  width: 12px;
  height: 12px;
  border-radius: 2px;
}

.legend-name {
  min-width: 100px;
}

.legend-value {
  color: var(--n-text-color-3);
}

.popular-repos {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.popular-repo {
  padding: 8px 12px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
  cursor: pointer;
  font-family: monospace;
  font-size: 12px;
  transition: all 0.2s;

  &:hover {
    border-color: var(--n-primary-color);
    background: var(--n-primary-color-suppl);
  }
}
</style>
