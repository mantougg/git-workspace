<template>
  <div class="regex-cheatsheet-tool">
    <div class="section">
      <div class="section-title">搜索正则模式</div>
      <n-input
        v-model:value="searchQuery"
        placeholder="搜索模式名称或描述..."
        clearable
      />
    </div>

    <div class="section">
      <div class="section-title">分类筛选</div>
      <n-space>
        <n-tag
          v-for="category in categories"
          :key="category"
          :type="selectedCategory === category ? 'primary' : 'default'"
          checkable
          :checked="selectedCategory === category"
          @update:checked="toggleCategory(category)"
        >
          {{ category }}
        </n-tag>
      </n-space>
    </div>

    <div class="section">
      <div class="section-title">标志说明</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>标志</th>
            <th>说明</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="flag in flags" :key="flag.flag">
            <td><n-code :code="flag.flag" language="text" /></td>
            <td>{{ flag.description }}</td>
          </tr>
        </tbody>
      </n-table>
    </div>

    <div class="section">
      <div class="section-title">模式列表</div>
      <div class="patterns-list">
        <div
          v-for="pattern in filteredPatterns"
          :key="pattern.pattern"
          class="pattern-item"
        >
          <div class="pattern-header">
            <n-code :code="pattern.pattern" language="regex" />
            <n-tag size="small">{{ pattern.category }}</n-tag>
          </div>
          <div class="pattern-description">{{ pattern.description }}</div>
          <div class="pattern-example">
            <div class="example-label">示例：</div>
            <n-code :code="pattern.example" language="text" />
          </div>
          <div class="pattern-matches">
            <div class="matches-label">匹配示例：</div>
            <n-space>
              <n-tag
                v-for="match in pattern.matches"
                :key="match"
                size="small"
                type="success"
              >
                {{ match }}
              </n-tag>
            </n-space>
          </div>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">常用正则表达式</div>
      <div class="snippets-list">
        <div
          v-for="snippet in snippets"
          :key="snippet.title"
          class="snippet-item"
        >
          <div class="snippet-title">{{ snippet.title }}</div>
          <div class="snippet-description">{{ snippet.description }}</div>
          <n-code :code="snippet.pattern" language="regex" />
          <div class="snippet-example">
            <span class="example-label">测试：</span>
            <n-code :code="snippet.example" language="text" />
          </div>
          <n-button size="small" @click="copyToClipboard(snippet.pattern)" style="margin-top: 8px">
            复制正则
          </n-button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'
import { regexPatterns, regexCategories, regexFlags } from '../data/regexCheatSheet'

const message = useMessage()
const searchQuery = ref('')
const selectedCategory = ref('')

const categories = regexCategories
const flags = regexFlags

const filteredPatterns = computed(() => {
  let result = regexPatterns

  if (selectedCategory.value) {
    result = result.filter(p => p.category === selectedCategory.value)
  }

  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(p =>
      p.description.toLowerCase().includes(query) ||
      p.pattern.toLowerCase().includes(query)
    )
  }

  return result
})

function toggleCategory(category: string) {
  selectedCategory.value = selectedCategory.value === category ? '' : category
}

const snippets = [
  {
    title: '邮箱验证',
    description: '验证邮箱地址格式',
    pattern: '^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$',
    example: 'user@example.com',
  },
  {
    title: '手机号验证（中国大陆）',
    description: '验证11位手机号',
    pattern: '^1[3-9]\\d{9}$',
    example: '13812345678',
  },
  {
    title: 'URL验证',
    description: '验证HTTP/HTTPS URL',
    pattern: '^https?:\\/\\/[^\\s]+',
    example: 'https://www.example.com/path',
  },
  {
    title: 'IP地址验证',
    description: '验证IPv4地址',
    pattern: '^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$',
    example: '192.168.1.1',
  },
  {
    title: '日期格式验证',
    description: '验证YYYY-MM-DD格式',
    pattern: '^\\d{4}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12]\\d|3[01])$',
    example: '2024-01-15',
  },
  {
    title: '密码强度',
    description: '至少8位，包含大小写字母和数字',
    pattern: '^(?=.*[a-z])(?=.*[A-Z])(?=.*\\d)[a-zA-Z\\d]{8,}$',
    example: 'Password123',
  },
  {
    title: '中文字符',
    description: '匹配中文字符',
    pattern: '[\\u4e00-\\u9fa5]+',
    example: '你好世界',
  },
  {
    title: 'HTML标签提取',
    description: '提取HTML标签内容',
    pattern: '<([a-z]+)[^>]*>(.*?)<\\/\\1>',
    example: '<div>content</div>',
  },
  {
    title: '去除首尾空格',
    description: '匹配首尾空格',
    pattern: '^\\s+|\\s+$',
    example: '  hello world  ',
  },
  {
    title: '千分位格式化',
    description: '数字千分位分隔',
    pattern: '(?<=\\d)(?=(\\d{3})+$)',
    example: '1234567 → 1,234,567',
  },
]

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.regex-cheatsheet-tool {
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

.patterns-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.pattern-item {
  padding: 16px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.pattern-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.pattern-description {
  color: var(--n-text-color-3);
  margin-bottom: 12px;
}

.pattern-example {
  margin-bottom: 8px;
}

.example-label {
  font-size: 12px;
  color: var(--n-text-color-3);
  margin-bottom: 4px;
}

.pattern-matches {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.matches-label {
  font-size: 12px;
  color: var(--n-text-color-3);
}

.snippets-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.snippet-item {
  padding: 16px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.snippet-title {
  font-weight: 500;
  margin-bottom: 4px;
}

.snippet-description {
  color: var(--n-text-color-3);
  font-size: 12px;
  margin-bottom: 8px;
}

.snippet-example {
  margin-top: 8px;
}
</style>
