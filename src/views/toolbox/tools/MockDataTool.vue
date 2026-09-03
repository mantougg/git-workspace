<template>
  <div class="mock-data-tool">
    <div class="section">
      <div class="section-title">生成选项</div>
      <div class="options-row">
        <div class="option">
          <label>数据类型</label>
          <n-select v-model:value="dataType" :options="dataTypeOptions" style="width: 150px" />
        </div>
        <div class="option">
          <label>数量</label>
          <n-input-number v-model:value="count" :min="1" :max="100" style="width: 100px" />
        </div>
        <div class="option">
          <label>输出格式</label>
          <n-select v-model:value="outputFormat" :options="formatOptions" style="width: 120px" />
        </div>
      </div>
      <n-button type="primary" @click="generate" style="margin-top: 12px">
        生成数据
      </n-button>
    </div>

    <div class="section">
      <div class="section-title">自定义字段</div>
      <div class="fields-list">
        <div v-for="(field, index) in customFields" :key="index" class="field-item">
          <n-input v-model:value="field.name" placeholder="字段名" style="width: 120px" />
          <n-select v-model:value="field.type" :options="fieldTypeOptions" style="width: 120px" />
          <n-button size="small" @click="removeField(index)">删除</n-button>
        </div>
      </div>
      <n-button size="small" @click="addField" style="margin-top: 8px">
        添加字段
      </n-button>
    </div>

    <div class="section">
      <div class="section-title">生成结果</div>
      <n-input
        v-model:value="generatedData"
        type="textarea"
        readonly
        :rows="12"
        placeholder="点击生成按钮..."
      />
      <div class="result-actions">
        <n-button @click="copyToClipboard(generatedData)">
          复制数据
        </n-button>
        <n-button @click="downloadData">
          下载文件
        </n-button>
        <n-button @click="clear">
          清空
        </n-button>
      </div>
    </div>

    <div class="section">
      <div class="section-title">预设模板</div>
      <div class="template-list">
        <div
          v-for="template in templates"
          :key="template.name"
          class="template-item"
          @click="applyTemplate(template)"
        >
          <div class="template-name">{{ template.name }}</div>
          <div class="template-description">{{ template.description }}</div>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const dataType = ref('person')
const count = ref(10)
const outputFormat = ref('json')
const generatedData = ref('')

const customFields = ref([
  { name: 'name', type: 'name' },
  { name: 'email', type: 'email' },
  { name: 'age', type: 'age' },
])

const dataTypeOptions = [
  { label: '人员信息', value: 'person' },
  { label: '地址信息', value: 'address' },
  { label: '公司信息', value: 'company' },
  { label: '商品信息', value: 'product' },
  { label: '自定义', value: 'custom' },
]

const formatOptions = [
  { label: 'JSON', value: 'json' },
  { label: 'CSV', value: 'csv' },
  { label: 'SQL', value: 'sql' },
]

const fieldTypeOptions = [
  { label: '姓名', value: 'name' },
  { label: '邮箱', value: 'email' },
  { label: '电话', value: 'phone' },
  { label: '年龄', value: 'age' },
  { label: '地址', value: 'address' },
  { label: '公司', value: 'company' },
  { label: '职位', value: 'job' },
  { label: '日期', value: 'date' },
  { label: '数字', value: 'number' },
  { label: '文本', value: 'text' },
  { label: '布尔值', value: 'boolean' },
  { label: 'UUID', value: 'uuid' },
]

const templates = [
  {
    name: '用户列表',
    description: '生成用户信息数据',
    fields: [
      { name: 'id', type: 'uuid' },
      { name: 'name', type: 'name' },
      { name: 'email', type: 'email' },
      { name: 'phone', type: 'phone' },
      { name: 'age', type: 'age' },
    ],
  },
  {
    name: '商品列表',
    description: '生成商品信息数据',
    fields: [
      { name: 'id', type: 'uuid' },
      { name: 'name', type: 'text' },
      { name: 'price', type: 'number' },
      { name: 'stock', type: 'number' },
      { name: 'createdAt', type: 'date' },
    ],
  },
  {
    name: '订单列表',
    description: '生成订单信息数据',
    fields: [
      { name: 'orderId', type: 'uuid' },
      { name: 'customer', type: 'name' },
      { name: 'amount', type: 'number' },
      { name: 'status', type: 'boolean' },
      { name: 'orderDate', type: 'date' },
    ],
  },
]

function addField() {
  customFields.value.push({ name: '', type: 'text' })
}

function removeField(index: number) {
  customFields.value.splice(index, 1)
}

function generate() {
  const data = []
  for (let i = 0; i < count.value; i++) {
    const item: Record<string, any> = {}
    for (const field of customFields.value) {
      item[field.name] = generateFieldValue(field.type)
    }
    data.push(item)
  }

  switch (outputFormat.value) {
    case 'json':
      generatedData.value = JSON.stringify(data, null, 2)
      break
    case 'csv':
      generatedData.value = generateCSV(data)
      break
    case 'sql':
      generatedData.value = generateSQL(data)
      break
  }
}

function generateFieldValue(type: string): any {
  switch (type) {
    case 'name':
      return generateName()
    case 'email':
      return generateEmail()
    case 'phone':
      return generatePhone()
    case 'age':
      return randomInt(18, 65)
    case 'address':
      return generateAddress()
    case 'company':
      return generateCompany()
    case 'job':
      return generateJob()
    case 'date':
      return generateDate()
    case 'number':
      return randomInt(1, 1000)
    case 'text':
      return generateText()
    case 'boolean':
      return Math.random() > 0.5
    case 'uuid':
      return generateUUID()
    default:
      return ''
  }
}

function generateName(): string {
  const firstNames = ['张', '李', '王', '赵', '刘', '陈', '杨', '黄', '吴', '周']
  const lastNames = ['伟', '芳', '秀英', '敏', '静', '丽', '强', '磊', '洋', '勇']
  return firstNames[randomInt(0, firstNames.length - 1)] + lastNames[randomInt(0, lastNames.length - 1)]
}

function generateEmail(): string {
  const domains = ['gmail.com', 'outlook.com', 'yahoo.com', 'hotmail.com', 'example.com']
  const names = ['user', 'test', 'demo', 'sample', 'mock']
  return `${names[randomInt(0, names.length - 1)]}${randomInt(100, 999)}@${domains[randomInt(0, domains.length - 1)]}`
}

function generatePhone(): string {
  const prefixes = ['138', '139', '150', '151', '152', '158', '159', '188', '189']
  return prefixes[randomInt(0, prefixes.length - 1)] + String(randomInt(10000000, 99999999))
}

function generateAddress(): string {
  const cities = ['北京', '上海', '广州', '深圳', '杭州', '成都', '武汉', '南京']
  const districts = ['朝阳区', '海淀区', '浦东新区', '天河区', '南山区', '西湖区']
  const streets = ['人民路', '中山路', '解放路', '建设路', '科技路', '创新路']
  return `${cities[randomInt(0, cities.length - 1)]}市${districts[randomInt(0, districts.length - 1)]}${streets[randomInt(0, streets.length - 1)]}${randomInt(1, 100)}号`
}

function generateCompany(): string {
  const prefixes = ['北京', '上海', '广州', '深圳', '杭州']
  const names = ['科技', '网络', '信息', '智能', '创新']
  const suffixes = ['有限公司', '股份有限公司', '集团']
  return `${prefixes[randomInt(0, prefixes.length - 1)]}${names[randomInt(0, names.length - 1)]}${suffixes[randomInt(0, suffixes.length - 1)]}`
}

function generateJob(): string {
  const jobs = ['软件工程师', '产品经理', '设计师', '运营', '市场', '销售', '财务', '人力资源']
  return jobs[randomInt(0, jobs.length - 1)]
}

function generateDate(): string {
  const start = new Date(2020, 0, 1)
  const end = new Date()
  const date = new Date(start.getTime() + Math.random() * (end.getTime() - start.getTime()))
  return date.toISOString().split('T')[0]
}

function generateText(): string {
  const texts = ['这是一段测试文本', '用于演示数据生成', '随机文本内容', '示例数据']
  return texts[randomInt(0, texts.length - 1)]
}

function generateUUID(): string {
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0
    const v = c === 'x' ? r : (r & 0x3) | 0x8
    return v.toString(16)
  })
}

function randomInt(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min
}

function generateCSV(data: Record<string, any>[]): string {
  if (data.length === 0) return ''
  const headers = Object.keys(data[0])
  const rows = data.map(row => headers.map(h => JSON.stringify(row[h])).join(','))
  return [headers.join(','), ...rows].join('\n')
}

function generateSQL(data: Record<string, any>[]): string {
  if (data.length === 0) return ''
  const tableName = 'mock_data'
  const columns = Object.keys(data[0])
  const inserts = data.map(row => {
    const values = columns.map(col => {
      const val = row[col]
      if (typeof val === 'string') return `'${val}'`
      if (typeof val === 'boolean') return val ? 'TRUE' : 'FALSE'
      return val
    })
    return `INSERT INTO ${tableName} (${columns.join(', ')}) VALUES (${values.join(', ')});`
  })
  return inserts.join('\n')
}

function applyTemplate(template: any) {
  customFields.value = [...template.fields]
  generate()
}

function clear() {
  generatedData.value = ''
}

function copyToClipboard(text: string) {
  if (!text) return
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}

function downloadData() {
  if (!generatedData.value) return
  const ext = outputFormat.value === 'json' ? 'json' : outputFormat.value === 'csv' ? 'csv' : 'sql'
  const blob = new Blob([generatedData.value], { type: 'text/plain' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `mock_data.${ext}`
  a.click()
  URL.revokeObjectURL(url)
}
</script>

<style scoped lang="scss">
.mock-data-tool {
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

.options-row {
  display: flex;
  gap: 16px;
  flex-wrap: wrap;
  align-items: flex-end;
}

.option {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.option label {
  font-size: 12px;
  color: var(--n-text-color-3);
}

.fields-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.field-item {
  display: flex;
  gap: 8px;
  align-items: center;
}

.result-actions {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}

.template-list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 8px;
}

.template-item {
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

.template-name {
  font-weight: 500;
  margin-bottom: 4px;
}

.template-description {
  font-size: 12px;
  color: var(--n-text-color-3);
}
</style>
