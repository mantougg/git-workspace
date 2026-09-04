<template>
  <div class="regex-tester-tool">
    <div class="regex-input-section">
      <div class="regex-row">
        <n-input
          v-model:value="regexPattern"
          placeholder="输入正则表达式..."
          @input="testRegex"
        >
          <template #prefix>/</template>
          <template #suffix>/{{ regexFlags }}</template>
        </n-input>
        <n-input
          v-model:value="regexFlags"
          placeholder="标志"
          style="width: 80px"
          @input="testRegex"
        />
      </div>
      <n-input
        v-model:value="testString"
        type="textarea"
        placeholder="输入测试文本..."
        :rows="4"
        @input="testRegex"
      />
    </div>

    <div class="flags-section">
      <n-space>
        <n-checkbox v-model:checked="flagG" @update:checked="updateFlags">g (全局)</n-checkbox>
        <n-checkbox v-model:checked="flagI" @update:checked="updateFlags">i (忽略大小写)</n-checkbox>
        <n-checkbox v-model:checked="flagM" @update:checked="updateFlags">m (多行)</n-checkbox>
        <n-checkbox v-model:checked="flagS" @update:checked="updateFlags">s (点号匹配换行)</n-checkbox>
      </n-space>
    </div>

    <div v-if="error" class="error-section">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="matches.length > 0" class="result-section">
      <n-descriptions bordered :column="1">
        <n-descriptions-item label="匹配数量">
          {{ matches.length }}
        </n-descriptions-item>
      </n-descriptions>

      <div class="matches-list">
        <div v-for="(match, index) in matches" :key="index" class="match-item">
          <div class="match-header">
            <span class="match-index">匹配 {{ index + 1 }}</span>
            <span class="match-position">
              位置: {{ match.index }} - {{ (match.index ?? 0) + match[0].length }}
            </span>
          </div>
          <div class="match-content">
            <n-code :code="match[0]" language="text" />
          </div>
          <div v-if="match.length > 1" class="capture-groups">
            <div v-for="(group, groupIndex) in match.slice(1)" :key="groupIndex" class="capture-group">
              <span class="group-label">捕获组 {{ groupIndex + 1 }}:</span>
              <span class="group-value">{{ group || '(空)' }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <div v-else-if="regexPattern && testString && !error" class="no-match">
      <n-alert type="warning" title="没有匹配" />
    </div>

    <div class="highlighted-section">
      <div class="section-title">高亮显示</div>
      <div class="highlighted-text" v-html="highlightedText" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'

const regexPattern = ref('')
const regexFlags = ref('g')
const testString = ref('')
const error = ref('')
const matches = ref<RegExpMatchArray[]>([])

const flagG = ref(true)
const flagI = ref(false)
const flagM = ref(false)
const flagS = ref(false)

function updateFlags() {
  let flags = ''
  if (flagG.value) flags += 'g'
  if (flagI.value) flags += 'i'
  if (flagM.value) flags += 'm'
  if (flagS.value) flags += 's'
  regexFlags.value = flags
  testRegex()
}

function testRegex() {
  error.value = ''
  matches.value = []

  if (!regexPattern.value || !testString.value) {
    return
  }

  try {
    const regex = new RegExp(regexPattern.value, regexFlags.value)
    const allMatches: RegExpMatchArray[] = []

    if (regexFlags.value.includes('g')) {
      let match: RegExpMatchArray | null
      while ((match = regex.exec(testString.value)) !== null) {
        allMatches.push(match)
        if (match.index === regex.lastIndex) {
          regex.lastIndex++
        }
      }
    } else {
      const match = regex.exec(testString.value)
      if (match) {
        allMatches.push(match)
      }
    }

    matches.value = allMatches
  } catch (e: any) {
    error.value = e.message
  }
}

const highlightedText = computed(() => {
  if (!regexPattern.value || !testString.value || matches.value.length === 0) {
    return testString.value
  }

  try {
    const regex = new RegExp(regexPattern.value, regexFlags.value)
    return testString.value.replace(regex, '<mark>$&</mark>')
  } catch {
    return testString.value
  }
})
</script>

<style scoped lang="scss">
.regex-tester-tool {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.regex-input-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.regex-row {
  display: flex;
  gap: 8px;
}

.flags-section {
  padding: 8px 0;
}

.error-section {
  margin-top: 8px;
}

.result-section {
  margin-top: 16px;
}

.matches-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.match-item {
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
  padding: 12px;
}

.match-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 8px;
}

.match-index {
  font-weight: 500;
  color: var(--n-primary-color);
}

.match-position {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.match-content {
  background: var(--n-color);
  padding: 8px;
  border-radius: 4px;
}

.capture-groups {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px dashed var(--n-border-color);
}

.capture-group {
  display: flex;
  gap: 8px;
  margin-bottom: 4px;
}

.group-label {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.group-value {
  font-family: monospace;
}

.no-match {
  margin-top: 16px;
}

.highlighted-section {
  margin-top: 16px;
}

.section-title {
  font-weight: 500;
  margin-bottom: 8px;
}

.highlighted-text {
  background: var(--n-color);
  padding: 12px;
  border-radius: 4px;
  white-space: pre-wrap;
  word-break: break-all;

  :deep(mark) {
    background: var(--n-primary-color-suppl);
    color: var(--n-primary-color);
    padding: 2px 4px;
    border-radius: 2px;
  }
}
</style>
