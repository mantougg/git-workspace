<template>
  <div class="unicode-codec-tool">
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
        </div>
        <div class="section">
          <div class="section-title">编码格式</div>
          <n-radio-group v-model:value="encodeFormat">
            <n-space>
              <n-radio value="unicode">Unicode (\uXXXX)</n-radio>
              <n-radio value="html">HTML (&#xXXXX;)</n-radio>
              <n-radio value="css">CSS (\XXXX)</n-radio>
              <n-radio value="js">JavaScript (\uXXXX)</n-radio>
            </n-space>
          </n-radio-group>
        </div>
        <n-button type="primary" @click="encode" style="margin-top: 8px">
          编码
        </n-button>
        <div class="section" v-if="encodedResult">
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
          <div class="section-title">输入 Unicode</div>
          <n-input
            v-model:value="inputUnicode"
            type="textarea"
            placeholder="输入 Unicode 编码（如 \u4f60\u597d 或 &#x4F60;&#x597D;）..."
            :rows="4"
          />
        </div>
        <n-button type="primary" @click="decode" style="margin-top: 8px">
          解码
        </n-button>
        <div class="section" v-if="decodedResult">
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

    <div class="char-info">
      <div class="section-title">字符信息</div>
      <n-input
        v-model:value="charInput"
        placeholder="输入一个字符查看详细信息"
        @input="analyzeChar"
      />
      <div v-if="charInfo" class="char-details">
        <n-descriptions bordered :column="2">
          <n-descriptions-item label="字符">{{ charInfo.char }}</n-descriptions-item>
          <n-descriptions-item label="Unicode 码点">U+{{ charInfo.codePoint }}</n-descriptions-item>
          <n-descriptions-item label="HTML 实体">{{ charInfo.htmlEntity }}</n-descriptions-item>
          <n-descriptions-item label="JavaScript">{{ charInfo.jsEscape }}</n-descriptions-item>
          <n-descriptions-item label="UTF-8 (十六进制)">{{ charInfo.utf8Hex }}</n-descriptions-item>
          <n-descriptions-item label="UTF-8 (十进制)">{{ charInfo.utf8Decimal }}</n-descriptions-item>
        </n-descriptions>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const inputText = ref('')
const encodedResult = ref('')
const inputUnicode = ref('')
const decodedResult = ref('')
const encodeFormat = ref('unicode')
const charInput = ref('')
const charInfo = ref<any>(null)

function encode() {
  if (!inputText.value) return
  let result = ''
  for (const char of inputText.value) {
    const codePoint = char.codePointAt(0)!
    switch (encodeFormat.value) {
      case 'unicode':
        result += `\\u${codePoint.toString(16).padStart(4, '0')}`
        break
      case 'html':
        result += `&#x${codePoint.toString(16).toUpperCase()};`
        break
      case 'css':
        result += `\\${codePoint.toString(16).toUpperCase().padStart(4, '0')}`
        break
      case 'js':
        result += `\\u${codePoint.toString(16).padStart(4, '0')}`
        break
    }
  }
  encodedResult.value = result
}

function decode() {
  if (!inputUnicode.value) return
  try {
    let result = inputUnicode.value
    // 处理 \uXXXX 格式
    result = result.replace(/\\u([0-9a-fA-F]{4})/g, (_, hex) => {
      return String.fromCodePoint(parseInt(hex, 16))
    })
    // 处理 &#xXXXX; 格式
    result = result.replace(/&#x([0-9a-fA-F]+);/g, (_, hex) => {
      return String.fromCodePoint(parseInt(hex, 16))
    })
    // 处理 &#DDDD; 格式
    result = result.replace(/&#(\d+);/g, (_, dec) => {
      return String.fromCodePoint(parseInt(dec, 10))
    })
    decodedResult.value = result
  } catch (e: any) {
    message.error('解码失败：' + e.message)
  }
}

function analyzeChar() {
  if (!charInput.value) {
    charInfo.value = null
    return
  }
  const char = charInput.value[0]
  const codePoint = char.codePointAt(0)!
  const hex = codePoint.toString(16).toUpperCase().padStart(4, '0')

  // UTF-8 编码
  const encoder = new TextEncoder()
  const utf8Bytes = encoder.encode(char)
  const utf8Hex = Array.from(utf8Bytes).map(b => b.toString(16).toUpperCase().padStart(2, '0')).join(' ')
  const utf8Decimal = Array.from(utf8Bytes).join(' ')

  charInfo.value = {
    char,
    codePoint: hex,
    htmlEntity: `&#x${hex};`,
    jsEscape: `\\u${hex}`,
    utf8Hex,
    utf8Decimal,
  }
}

function copyToClipboard(text: string) {
  if (!text) return
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.unicode-codec-tool {
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

.char-info {
  margin-top: 16px;
  padding-top: 16px;
  border-top: 1px solid var(--n-border-color);
}

.char-details {
  margin-top: 12px;
}
</style>
