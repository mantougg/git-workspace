<template>
  <div class="css-cheatsheet-tool">
    <div class="section">
      <div class="section-title">搜索 CSS 属性</div>
      <n-input
        v-model:value="searchQuery"
        placeholder="搜索属性名称或描述..."
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
      <div class="section-title">属性列表</div>
      <div class="properties-list">
        <div
          v-for="prop in filteredProperties"
          :key="prop.property"
          class="property-item"
        >
          <div class="property-header">
            <span class="property-name">{{ prop.property }}</span>
            <n-tag size="small">{{ prop.category }}</n-tag>
          </div>
          <div class="property-description">{{ prop.description }}</div>
          <div class="property-values">
            <span class="values-label">可选值：</span>
            <n-space>
              <n-tag
                v-for="value in prop.values"
                :key="value"
                size="small"
                type="info"
              >
                {{ value }}
              </n-tag>
            </n-space>
          </div>
          <div class="property-example">
            <n-code :code="prop.example" language="css" />
          </div>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">常用代码片段</div>
      <div class="snippets-list">
        <div
          v-for="snippet in snippets"
          :key="snippet.title"
          class="snippet-item"
        >
          <div class="snippet-title">{{ snippet.title }}</div>
          <div class="snippet-description">{{ snippet.description }}</div>
          <n-code :code="snippet.code" language="css" />
          <n-button size="small" @click="copyToClipboard(snippet.code)">
            复制
          </n-button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'
import { cssProperties, cssCategories } from '../data/cssCheatSheet'

const message = useMessage()
const searchQuery = ref('')
const selectedCategory = ref('')

const categories = cssCategories

const filteredProperties = computed(() => {
  let result = cssProperties

  if (selectedCategory.value) {
    result = result.filter(p => p.category === selectedCategory.value)
  }

  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(p =>
      p.property.toLowerCase().includes(query) ||
      p.description.toLowerCase().includes(query)
    )
  }

  return result
})

function toggleCategory(category: string) {
  selectedCategory.value = selectedCategory.value === category ? '' : category
}

const snippets = [
  {
    title: '水平垂直居中',
    description: '使用 Flexbox 实现水平垂直居中',
    code: `.center {
  display: flex;
  justify-content: center;
  align-items: center;
}`,
  },
  {
    title: '文字溢出省略号',
    description: '单行文字溢出显示省略号',
    code: `.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}`,
  },
  {
    title: '多行文字溢出省略号',
    description: '多行文字溢出显示省略号（Webkit）',
    code: `.multiline-ellipsis {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  overflow: hidden;
}`,
  },
  {
    title: '清除浮动',
    description: '使用 clearfix 清除浮动',
    code: `.clearfix::after {
  content: "";
  display: table;
  clear: both;
}`,
  },
  {
    title: '响应式图片',
    description: '图片自适应容器大小',
    code: `.responsive-img {
  max-width: 100%;
  height: auto;
}`,
  },
  {
    title: 'CSS 三角形',
    description: '使用 CSS 创建三角形',
    code: `.triangle {
  width: 0;
  height: 0;
  border-left: 50px solid transparent;
  border-right: 50px solid transparent;
  border-bottom: 100px solid #333;
}`,
  },
  {
    title: '毛玻璃效果',
    description: '背景毛玻璃模糊效果',
    code: `.glass {
  background: rgba(255, 255, 255, 0.2);
  backdrop-filter: blur(10px);
  border: 1px solid rgba(255, 255, 255, 0.3);
}`,
  },
  {
    title: '渐变边框',
    description: '使用渐变色作为边框',
    code: `.gradient-border {
  border: 2px solid;
  border-image: linear-gradient(to right, #667eea, #764ba2) 1;
}`,
  },
]

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.css-cheatsheet-tool {
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

.properties-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.property-item {
  padding: 16px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.property-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.property-name {
  font-family: monospace;
  font-weight: 500;
  color: var(--n-primary-color);
  font-size: 16px;
}

.property-description {
  color: var(--n-text-color-3);
  margin-bottom: 8px;
}

.property-values {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  flex-wrap: wrap;
}

.values-label {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.property-example {
  background: var(--n-color);
  padding: 8px;
  border-radius: 4px;
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
</style>
