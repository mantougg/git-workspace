<template>
  <div class="html-entity-tool">
    <n-tabs type="segment" animated>
      <n-tab-pane name="encode" tab="编码">
        <div class="section">
          <div class="section-title">输入文本</div>
          <n-input
            v-model:value="inputText"
            type="textarea"
            placeholder="输入要编码的文本..."
            :rows="4"
          />
          <n-button type="primary" @click="encode" style="margin-top: 8px">
            编码
          </n-button>
        </div>
        <div class="section">
          <div class="section-title">编码结果</div>
          <n-input
            v-model:value="encodedResult"
            type="textarea"
            readonly
            :rows="4"
          />
          <n-button @click="copyToClipboard(encodedResult)" style="margin-top: 8px">
            复制
          </n-button>
        </div>
      </n-tab-pane>

      <n-tab-pane name="decode" tab="解码">
        <div class="section">
          <div class="section-title">输入 HTML 实体</div>
          <n-input
            v-model:value="inputEntity"
            type="textarea"
            placeholder="输入 HTML 实体（如 &amp;lt; 或 &#60;）..."
            :rows="4"
          />
          <n-button type="primary" @click="decode" style="margin-top: 8px">
            解码
          </n-button>
        </div>
        <div class="section">
          <div class="section-title">解码结果</div>
          <n-input
            v-model:value="decodedResult"
            type="textarea"
            readonly
            :rows="4"
          />
          <n-button @click="copyToClipboard(decodedResult)" style="margin-top: 8px">
            复制
          </n-button>
        </div>
      </n-tab-pane>
    </n-tabs>

    <div class="common-entities">
      <div class="section-title">常用 HTML 实体</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>字符</th>
            <th>实体名称</th>
            <th>实体编号</th>
            <th>描述</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entity in commonEntities" :key="entity.char">
            <td><n-code :code="entity.char" language="text" /></td>
            <td><n-code :code="entity.name" language="text" /></td>
            <td><n-code :code="entity.number" language="text" /></td>
            <td>{{ entity.description }}</td>
          </tr>
        </tbody>
      </n-table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const inputText = ref('')
const encodedResult = ref('')
const inputEntity = ref('')
const decodedResult = ref('')

const commonEntities = [
  { char: '<', name: '&lt;', number: '&#60;', description: '小于号' },
  { char: '>', name: '&gt;', number: '&#62;', description: '大于号' },
  { char: '&', name: '&amp;', number: '&#38;', description: '和号' },
  { char: '"', name: '&quot;', number: '&#34;', description: '双引号' },
  { char: "'", name: '&apos;', number: '&#39;', description: '单引号' },
  { char: ' ', name: '&nbsp;', number: '&#160;', description: '空格' },
  { char: '©', name: '&copy;', number: '&#169;', description: '版权符号' },
  { char: '®', name: '&reg;', number: '&#174;', description: '注册商标' },
  { char: '™', name: '&trade;', number: '&#8482;', description: '商标' },
  { char: '€', name: '&euro;', number: '&#8364;', description: '欧元' },
  { char: '£', name: '&pound;', number: '&#163;', description: '英镑' },
  { char: '¥', name: '&yen;', number: '&#165;', description: '日元/人民币' },
]

function encode() {
  if (!inputText.value) return
  encodedResult.value = inputText.value
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function decode() {
  if (!inputEntity.value) return
  const textarea = document.createElement('textarea')
  textarea.innerHTML = inputEntity.value
  decodedResult.value = textarea.value
}

function copyToClipboard(text: string) {
  if (!text) return
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.html-entity-tool {
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

.common-entities {
  margin-top: 16px;
}
</style>
