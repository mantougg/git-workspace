<template>
  <div class="linux-cheatsheet-tool">
    <div class="section">
      <div class="section-title">搜索命令</div>
      <n-input
        v-model:value="searchQuery"
        placeholder="搜索命令名称或描述..."
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
      <div class="section-title">命令列表</div>
      <div class="commands-list">
        <div
          v-for="cmd in filteredCommands"
          :key="cmd.command"
          class="command-item"
        >
          <div class="command-header">
            <span class="command-name">{{ cmd.command }}</span>
            <n-tag size="small">{{ cmd.category }}</n-tag>
          </div>
          <div class="command-description">{{ cmd.description }}</div>
          <div class="command-syntax">
            <div class="syntax-label">语法：</div>
            <n-code :code="cmd.syntax" language="bash" />
          </div>
          <div class="command-example">
            <div class="example-label">示例：</div>
            <n-code :code="cmd.example" language="bash" />
          </div>
          <n-button size="small" @click="copyToClipboard(cmd.example)" style="margin-top: 8px">
            复制示例
          </n-button>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">常用组合命令</div>
      <div class="snippets-list">
        <div
          v-for="snippet in snippets"
          :key="snippet.title"
          class="snippet-item"
        >
          <div class="snippet-title">{{ snippet.title }}</div>
          <div class="snippet-description">{{ snippet.description }}</div>
          <n-code :code="snippet.code" language="bash" />
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
import { linuxCommands, linuxCategories } from '../data/linuxCheatSheet'

const message = useMessage()
const searchQuery = ref('')
const selectedCategory = ref('')

const categories = linuxCategories

const filteredCommands = computed(() => {
  let result = linuxCommands

  if (selectedCategory.value) {
    result = result.filter(c => c.category === selectedCategory.value)
  }

  if (searchQuery.value) {
    const query = searchQuery.value.toLowerCase()
    result = result.filter(c =>
      c.command.toLowerCase().includes(query) ||
      c.description.toLowerCase().includes(query)
    )
  }

  return result
})

function toggleCategory(category: string) {
  selectedCategory.value = selectedCategory.value === category ? '' : category
}

const snippets = [
  {
    title: '查找大文件',
    description: '查找系统中大于100MB的文件',
    code: `find / -type f -size +100M -exec ls -lh {} \\; 2>/dev/null`,
  },
  {
    title: '批量重命名文件',
    description: '将当前目录下所有 .txt 文件改为 .md',
    code: `for f in *.txt; do mv "$f" "\${f%.txt}.md"; done`,
  },
  {
    title: '统计文件行数',
    description: '统计当前目录下所有 .js 文件的总行数',
    code: `find . -name "*.js" -exec wc -l {} + | tail -1`,
  },
  {
    title: '清理日志文件',
    description: '删除7天前的日志文件',
    code: `find /var/log -name "*.log" -mtime +7 -delete`,
  },
  {
    title: '监控日志文件',
    description: '实时监控日志文件并高亮显示错误',
    code: `tail -f /var/log/syslog | grep --color=auto -i "error"`,
  },
  {
    title: '批量杀进程',
    description: '杀掉所有匹配的进程',
    code: `ps aux | grep "process_name" | grep -v grep | awk '{print $2}' | xargs kill -9`,
  },
  {
    title: '查看端口占用',
    description: '查看指定端口被哪个进程占用',
    code: `lsof -i :8080`,
  },
  {
    title: '磁盘空间分析',
    description: '按大小排序显示当前目录下各文件夹大小',
    code: `du -sh * | sort -rh | head -20`,
  },
  {
    title: '查找重复文件',
    description: '基于MD5查找重复文件',
    code: `find . -type f -exec md5sum {} \\; | sort | uniq -d -w 32`,
  },
  {
    title: '批量替换文件内容',
    description: '批量替换当前目录下所有文件中的文本',
    code: `find . -type f -exec sed -i 's/old_text/new_text/g' {} +`,
  },
  {
    title: '系统资源监控',
    description: '每2秒刷新一次系统资源使用情况',
    code: `watch -n 2 'echo "=== CPU ===" && top -bn1 | head -5 && echo "=== Memory ===" && free -h && echo "=== Disk ===" && df -h'`,
  },
  {
    title: '网络连接统计',
    description: '统计各状态的TCP连接数量',
    code: `netstat -ant | awk '{print $6}' | sort | uniq -c | sort -rn`,
  },
]

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.linux-cheatsheet-tool {
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

.commands-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.command-item {
  padding: 16px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
}

.command-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.command-name {
  font-family: monospace;
  font-weight: 500;
  color: var(--n-primary-color);
  font-size: 16px;
}

.command-description {
  color: var(--n-text-color-3);
  margin-bottom: 12px;
}

.command-syntax {
  margin-bottom: 8px;
}

.syntax-label,
.example-label {
  font-size: 12px;
  color: var(--n-text-color-3);
  margin-bottom: 4px;
}

.command-example {
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
