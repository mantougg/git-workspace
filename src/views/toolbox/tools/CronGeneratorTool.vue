<template>
  <div class="cron-generator-tool">
    <div class="section">
      <div class="section-title">Cron 表达式</div>
      <n-input
        v-model:value="cronExpression"
        placeholder="* * * * *"
        readonly
        size="large"
      >
        <template #suffix>
          <n-button size="small" @click="copyToClipboard(cronExpression)">
            复制
          </n-button>
        </template>
      </n-input>
      <div class="cron-description">{{ cronDescription }}</div>
    </div>

    <div class="section">
      <div class="section-title">可视化配置</div>
      <div class="cron-fields">
        <div class="cron-field">
          <label>分钟 (0-59)</label>
          <n-input v-model:value="minute" placeholder="*" @input="updateCron" />
        </div>
        <div class="cron-field">
          <label>小时 (0-23)</label>
          <n-input v-model:value="hour" placeholder="*" @input="updateCron" />
        </div>
        <div class="cron-field">
          <label>日期 (1-31)</label>
          <n-input v-model:value="dayOfMonth" placeholder="*" @input="updateCron" />
        </div>
        <div class="cron-field">
          <label>月份 (1-12)</label>
          <n-input v-model:value="month" placeholder="*" @input="updateCron" />
        </div>
        <div class="cron-field">
          <label>星期 (0-6)</label>
          <n-input v-model:value="dayOfWeek" placeholder="*" @input="updateCron" />
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">常用表达式</div>
      <div class="preset-list">
        <div
          v-for="preset in presets"
          :key="preset.expression"
          class="preset-item"
          @click="applyPreset(preset)"
        >
          <div class="preset-expression">{{ preset.expression }}</div>
          <div class="preset-description">{{ preset.description }}</div>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">下次执行时间</div>
      <div class="next-runs">
        <div v-for="(run, index) in nextRuns" :key="index" class="next-run">
          {{ run }}
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">Cron 语法说明</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>字段</th>
            <th>范围</th>
            <th>特殊字符</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>分钟</td>
            <td>0-59</td>
            <td>* , - /</td>
          </tr>
          <tr>
            <td>小时</td>
            <td>0-23</td>
            <td>* , - /</td>
          </tr>
          <tr>
            <td>日期</td>
            <td>1-31</td>
            <td>* , - /</td>
          </tr>
          <tr>
            <td>月份</td>
            <td>1-12</td>
            <td>* , - /</td>
          </tr>
          <tr>
            <td>星期</td>
            <td>0-6 (0=周日)</td>
            <td>* , - /</td>
          </tr>
        </tbody>
      </n-table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const minute = ref('*')
const hour = ref('*')
const dayOfMonth = ref('*')
const month = ref('*')
const dayOfWeek = ref('*')

const cronExpression = computed(() => {
  return `${minute.value} ${hour.value} ${dayOfMonth.value} ${month.value} ${dayOfWeek.value}`
})

const cronDescription = computed(() => {
  return describeCron(cronExpression.value)
})

const presets = [
  { expression: '* * * * *', description: '每分钟' },
  { expression: '0 * * * *', description: '每小时' },
  { expression: '0 0 * * *', description: '每天午夜' },
  { expression: '0 0 * * 1', description: '每周一午夜' },
  { expression: '0 0 1 * *', description: '每月1号午夜' },
  { expression: '0 0 1 1 *', description: '每年1月1日午夜' },
  { expression: '*/5 * * * *', description: '每5分钟' },
  { expression: '0 */2 * * *', description: '每2小时' },
  { expression: '0 9 * * 1-5', description: '工作日上午9点' },
  { expression: '0 0 * * 0', description: '每周日午夜' },
  { expression: '30 4 * * *', description: '每天凌晨4:30' },
  { expression: '0 22 * * 1-5', description: '工作日晚上10点' },
]

const nextRuns = computed(() => {
  return getNextRuns(cronExpression.value, 5)
})

function updateCron() {
  // Cron expression is computed automatically
}

function applyPreset(preset: any) {
  const parts = preset.expression.split(' ')
  minute.value = parts[0]
  hour.value = parts[1]
  dayOfMonth.value = parts[2]
  month.value = parts[3]
  dayOfWeek.value = parts[4]
}

function describeCron(expr: string): string {
  const parts = expr.split(' ')
  if (parts.length !== 5) return '无效的 Cron 表达式'

  const [min, hour, dom, mon, dow] = parts

  let desc = ''

  // 分钟
  if (min === '*') {
    desc += '每分钟'
  } else if (min.startsWith('*/')) {
    desc += `每${min.slice(2)}分钟`
  } else if (min.includes(',')) {
    desc += `在第${min}分钟`
  } else {
    desc += `在第${min}分钟`
  }

  // 小时
  if (hour === '*') {
    desc += '的每小时'
  } else if (hour.startsWith('*/')) {
    desc += `的每${hour.slice(2)}小时`
  } else {
    desc += `的${hour}点`
  }

  // 日期
  if (dom === '*') {
    desc += '的每天'
  } else if (dom.startsWith('*/')) {
    desc += `的每${dom.slice(2)}天`
  } else {
    desc += `的${dom}号`
  }

  // 月份
  if (mon === '*') {
    desc += '的每月'
  } else if (mon.startsWith('*/')) {
    desc += `的每${mon.slice(2)}个月`
  } else {
    desc += `的${mon}月`
  }

  // 星期
  if (dow === '*') {
    desc += ''
  } else {
    const days = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
    if (dow.includes('-')) {
      const [start, end] = dow.split('-').map(Number)
      desc += `，${days[start]}到${days[end]}`
    } else if (dow.includes(',')) {
      const dayNames = dow.split(',').map(d => days[Number(d)])
      desc += `，${dayNames.join('和')}`
    } else {
      desc += `，${days[Number(dow)]}`
    }
  }

  return desc
}

function getNextRuns(expr: string, count: number): string[] {
  // 简化版：显示未来几个整点时间
  const now = new Date()
  const runs: string[] = []

  // 解析 cron 表达式
  const parts = expr.split(' ')
  if (parts.length !== 5) return []

  const [minExpr, hourExpr, domExpr, monExpr, dowExpr] = parts

  // 生成下几个执行时间（简化版）
  let current = new Date(now)
  current.setSeconds(0)
  current.setMilliseconds(0)

  for (let i = 0; i < count; i++) {
    current = getNextRunTime(current, minExpr, hourExpr, domExpr, monExpr, dowExpr)
    runs.push(formatDateTime(current))
    current = new Date(current.getTime() + 1000) // 加1秒避免重复
  }

  return runs
}

function getNextRunTime(
  from: Date,
  minExpr: string,
  hourExpr: string,
  domExpr: string,
  monExpr: string,
  dowExpr: string
): Date {
  let next = new Date(from)

  // 简化处理：只处理常见模式
  if (minExpr === '*' && hourExpr === '*') {
    // 每分钟
    next.setMinutes(next.getMinutes() + 1)
    next.setSeconds(0)
    next.setMilliseconds(0)
  } else if (minExpr.startsWith('*/')) {
    // 每N分钟
    const interval = parseInt(minExpr.slice(2))
    const currentMin = next.getMinutes()
    const nextMin = Math.ceil((currentMin + 1) / interval) * interval
    next.setMinutes(nextMin)
    next.setSeconds(0)
    next.setMilliseconds(0)
    if (nextMin >= 60) {
      next.setHours(next.getHours() + 1)
      next.setMinutes(nextMin - 60)
    }
  } else if (hourExpr.startsWith('*/')) {
    // 每N小时
    const interval = parseInt(hourExpr.slice(2))
    const currentHour = next.getHours()
    const nextHour = Math.ceil((currentHour + 1) / interval) * interval
    next.setHours(nextHour)
    next.setMinutes(parseInt(minExpr) || 0)
    next.setSeconds(0)
    next.setMilliseconds(0)
    if (nextHour >= 24) {
      next.setDate(next.getDate() + 1)
      next.setHours(nextHour - 24)
    }
  } else {
    // 固定时间
    next.setHours(parseInt(hourExpr) || 0)
    next.setMinutes(parseInt(minExpr) || 0)
    next.setSeconds(0)
    next.setMilliseconds(0)
    if (next <= from) {
      next.setDate(next.getDate() + 1)
    }
  }

  return next
}

function formatDateTime(date: Date): string {
  return date.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function copyToClipboard(text: string) {
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.cron-generator-tool {
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

.cron-description {
  margin-top: 8px;
  color: var(--n-text-color-3);
  font-size: 14px;
}

.cron-fields {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}

.cron-field {
  flex: 1;
  min-width: 120px;
}

.cron-field label {
  display: block;
  margin-bottom: 4px;
  font-size: 12px;
  color: var(--n-text-color-3);
}

.preset-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 8px;
}

.preset-item {
  padding: 12px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.2s;

  &:hover {
    border-color: var(--n-primary-color);
    background: var(--n-primary-color-suppl);
  }
}

.preset-expression {
  font-family: monospace;
  font-weight: 500;
  color: var(--n-primary-color);
}

.preset-description {
  margin-top: 4px;
  font-size: 12px;
  color: var(--n-text-color-3);
}

.next-runs {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.next-run {
  padding: 8px;
  background: var(--n-color);
  border-radius: 4px;
  font-family: monospace;
}
</style>
