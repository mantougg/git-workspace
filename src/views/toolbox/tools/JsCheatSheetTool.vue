<template>
  <div class="js-cheatsheet-tool">
    <div class="section">
      <div class="section-title">搜索方法</div>
      <n-input
        v-model:value="searchQuery"
        placeholder="搜索方法名称或描述..."
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
      <div class="section-title">方法列表</div>
      <div class="methods-list">
        <div
          v-for="method in filteredMethods"
          :key="method.name"
          class="method-item"
        >
          <div class="method-header">
            <span class="method-name">{{ method.name }}</span>
            <n-tag size="small">{{ method.category }}</n-tag>
          </div>
          <div class="method-description">{{ method.description }}</div>
          <div class="method-syntax">
            <div class="syntax-label">语法：</div>
            <n-code :code="method.syntax" language="javascript" />
          </div>
          <div class="method-example">
            <div class="example-label">示例：</div>
            <n-code :code="method.example" language="javascript" />
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
          <n-code :code="snippet.code" language="javascript" />
          <n-button size="small" @click="copyToClipboard(snippet.code)" style="margin-top: 8px">
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
import { jsMethods, jsCategories } from '../data/jsCheatSheet'

const message = useMessage()
const searchQuery = ref('')
const selectedCategory = ref('')

const categories = jsCategories

const filteredMethods = computed(() => {
  let result = jsMethods

  if (selectedCategory.value) {
    result = result.filter(m => m.category === selectedCategory.value)
  }

  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(m =>
      m.name.toLowerCase().includes(query) ||
      m.description.toLowerCase().includes(query)
    )
  }

  return result
})

function toggleCategory(category: string) {
  selectedCategory.value = selectedCategory.value === category ? '' : category
}

const snippets = [
  {
    title: '防抖函数',
    description: '延迟执行，多次触发只执行最后一次',
    code: `function debounce(fn, delay) {
  let timer = null;
  return function (...args) {
    clearTimeout(timer);
    timer = setTimeout(() => fn.apply(this, args), delay);
  };
}`,
  },
  {
    title: '节流函数',
    description: '限制函数执行频率',
    code: `function throttle(fn, limit) {
  let inThrottle = false;
  return function (...args) {
    if (!inThrottle) {
      fn.apply(this, args);
      inThrottle = true;
      setTimeout(() => inThrottle = false, limit);
    }
  };
}`,
  },
  {
    title: '深拷贝',
    description: '递归深拷贝对象',
    code: `function deepClone(obj) {
  if (obj === null || typeof obj !== 'object') return obj;
  const clone = Array.isArray(obj) ? [] : {};
  for (let key in obj) {
    if (obj.hasOwnProperty(key)) {
      clone[key] = deepClone(obj[key]);
    }
  }
  return clone;
}`,
  },
  {
    title: '数组去重',
    description: '使用 Set 去重',
    code: `const unique = arr => [...new Set(arr)];`,
  },
  {
    title: '数组分组',
    description: '按条件对数组进行分组',
    code: `function groupBy(arr, key) {
  return arr.reduce((groups, item) => {
    const group = typeof key === 'function' ? key(item) : item[key];
    groups[group] = groups[group] || [];
    groups[group].push(item);
    return groups;
  }, {});
}`,
  },
  {
    title: '异步并发控制',
    description: '限制并发请求数量',
    code: `async function asyncPool(limit, items, fn) {
  const results = [];
  const executing = [];
  for (const item of items) {
    const p = fn(item).then(result => {
      executing.splice(executing.indexOf(p), 1);
      return result;
    });
    results.push(p);
    executing.push(p);
    if (executing.length >= limit) {
      await Promise.race(executing);
    }
  }
  return Promise.all(results);
}`,
  },
  {
    title: '重试函数',
    description: '失败时自动重试',
    code: `async function retry(fn, retries = 3, delay = 1000) {
  for (let i = 0; i < retries; i++) {
    try {
      return await fn();
    } catch (err) {
      if (i === retries - 1) throw err;
      await new Promise(r => setTimeout(r, delay));
    }
  }
}`,
  },
  {
    title: 'Promise 超时控制',
    description: '为 Promise 添加超时',
    code: `function withTimeout(promise, ms) {
  const timeout = new Promise((_, reject) => {
    setTimeout(() => reject(new Error('Timeout')), ms);
  });
  return Promise.race([promise, timeout]);
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
.js-cheatsheet-tool {
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

.methods-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.method-item {
  padding: 16px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.method-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.method-name {
  font-family: monospace;
  font-weight: 500;
  color: var(--n-primary-color);
  font-size: 16px;
}

.method-description {
  color: var(--n-text-color-3);
  margin-bottom: 12px;
}

.method-syntax {
  margin-bottom: 8px;
}

.syntax-label,
.example-label {
  font-size: 12px;
  color: var(--n-text-color-3);
  margin-bottom: 4px;
}

.method-example {
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
