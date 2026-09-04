<template>
  <div class="regex-visualizer-tool">
    <div class="section">
      <div class="section-title">输入正则表达式</div>
      <div class="regex-input-row">
        <n-input
          v-model:value="regexPattern"
          placeholder="输入正则表达式..."
          @input="visualize"
        >
          <template #prefix>/</template>
          <template #suffix>/{{ regexFlags }}</template>
        </n-input>
        <n-input
          v-model:value="regexFlags"
          placeholder="标志"
          style="width: 80px"
          @input="visualize"
        />
      </div>
    </div>

    <div class="section">
      <div class="section-title">测试文本</div>
      <n-input
        v-model:value="testString"
        type="textarea"
        placeholder="输入测试文本..."
        :rows="3"
        @input="visualize"
      />
    </div>

    <div v-if="error" class="error-section">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="tokens.length > 0" class="section">
      <div class="section-title">表达式结构</div>
      <div class="tokens-tree">
        <div
          v-for="(token, index) in tokens"
          :key="index"
          class="token-item"
          :class="token.type"
        >
          <div class="token-header">
            <span class="token-type">{{ token.type }}</span>
            <span class="token-value">{{ token.value }}</span>
          </div>
          <div v-if="token.description" class="token-description">
            {{ token.description }}
          </div>
        </div>
      </div>
    </div>

    <div v-if="matches.length > 0" class="section">
      <div class="section-title">匹配结果</div>
      <div class="matches-visualization">
        <div class="text-display">
          <span
            v-for="(part, index) in highlightedParts"
            :key="index"
            :class="{ match: part.isMatch, group: part.isGroup }"
          >
            {{ part.text }}
          </span>
        </div>
      </div>
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

    <div class="section">
      <div class="section-title">正则语法说明</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>语法</th>
            <th>说明</th>
            <th>示例</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in syntaxGuide" :key="item.syntax">
            <td><n-code :code="item.syntax" language="text" /></td>
            <td>{{ item.description }}</td>
            <td><n-code :code="item.example" language="text" /></td>
          </tr>
        </tbody>
      </n-table>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'

const regexPattern = ref('')
const regexFlags = ref('g')
const testString = ref('')
const error = ref('')
const tokens = ref<any[]>([])
const matches = ref<RegExpMatchArray[]>([])

const syntaxGuide = [
  { syntax: '.', description: '匹配任意单个字符', example: 'a.c → abc, a1c' },
  { syntax: '\\d', description: '匹配数字', example: '\\d+ → 123' },
  { syntax: '\\w', description: '匹配单词字符', example: '\\w+ → hello' },
  { syntax: '\\s', description: '匹配空白字符', example: '\\s → 空格' },
  { syntax: '*', description: '匹配0次或多次', example: 'ab*c → ac, abc' },
  { syntax: '+', description: '匹配1次或多次', example: 'ab+c → abc' },
  { syntax: '?', description: '匹配0次或1次', example: 'colou?r → color' },
  { syntax: '{n}', description: '匹配n次', example: 'a{3} → aaa' },
  { syntax: '{n,m}', description: '匹配n到m次', example: 'a{2,4} → aa' },
  { syntax: '[abc]', description: '匹配字符集', example: '[aeiou] → a, e' },
  { syntax: '[^abc]', description: '匹配非字符集', example: '[^0-9] → a, b' },
  { syntax: '^', description: '匹配行首', example: '^Hello → Hello World' },
  { syntax: '$', description: '匹配行尾', example: 'World$ → Hello World' },
  { syntax: '(abc)', description: '捕获组', example: '(\\d+)-(\\d+)' },
  { syntax: '(?:abc)', description: '非捕获组', example: '(?:https?|ftp)' },
  { syntax: '(?=abc)', description: '正向前瞻', example: '\\d+(?=px)' },
  { syntax: '(?!abc)', description: '负向前瞻', example: '\\d+(?!px)' },
]

function visualize() {
  error.value = ''
  tokens.value = []
  matches.value = []

  if (!regexPattern.value) return

  try {
    // 解析正则表达式为 tokens
    tokens.value = parseRegex(regexPattern.value)

    // 执行匹配
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

function parseRegex(pattern: string): any[] {
  const tokens: any[] = []
  let i = 0

  while (i < pattern.length) {
    const char = pattern[i]

    // 转义字符
    if (char === '\\' && i + 1 < pattern.length) {
      const next = pattern[i + 1]
      const escapeMap: Record<string, string> = {
        'd': '数字 [0-9]',
        'D': '非数字',
        'w': '单词字符 [a-zA-Z0-9_]',
        'W': '非单词字符',
        's': '空白字符',
        'S': '非空白字符',
        'b': '单词边界',
        'B': '非单词边界',
        'n': '换行符',
        'r': '回车符',
        't': '制表符',
      }

      tokens.push({
        type: '转义',
        value: `\\${next}`,
        description: escapeMap[next] || `字面量 ${next}`,
      })
      i += 2
      continue
    }

    // 字符类
    if (char === '[') {
      let end = i + 1
      if (end < pattern.length && pattern[end] === '^') end++
      if (end < pattern.length && pattern[end] === ']') end++
      while (end < pattern.length && pattern[end] !== ']') end++
      if (end < pattern.length) end++

      const charClass = pattern.slice(i, end)
      const isNegated = charClass[1] === '^'
      tokens.push({
        type: '字符类',
        value: charClass,
        description: isNegated ? '匹配不在集合中的字符' : '匹配集合中的任意字符',
      })
      i = end
      continue
    }

    // 分组
    if (char === '(') {
      let groupType = '捕获组'
      let value = '('

      if (pattern[i + 1] === '?' && pattern[i + 2] === ':') {
        groupType = '非捕获组'
        value = '(?:'
        i += 2
      } else if (pattern[i + 1] === '?' && pattern[i + 2] === '=') {
        groupType = '正向前瞻'
        value = '(?='
        i += 2
      } else if (pattern[i + 1] === '?' && pattern[i + 2] === '!') {
        groupType = '负向前瞻'
        value = '(?!'
        i += 2
      } else if (pattern[i + 1] === '?' && pattern[i + 2] === '<' && pattern[i + 3] === '=') {
        groupType = '正向后顾'
        value = '(?<=',
        i += 3
      } else if (pattern[i + 1] === '?' && pattern[i + 2] === '<' && pattern[i + 3] === '!') {
        groupType = '负向后顾'
        value = '(?<!',
        i += 3
      }

      tokens.push({
        type: groupType,
        value,
        description: `开始${groupType}`,
      })
      i++
      continue
    }

    if (char === ')') {
      tokens.push({
        type: '分组结束',
        value: ')',
        description: '结束分组',
      })
      i++
      continue
    }

    // 量词
    if (char === '*' || char === '+' || char === '?') {
      const quantifierMap: Record<string, string> = {
        '*': '匹配0次或多次',
        '+': '匹配1次或多次',
        '?': '匹配0次或1次',
      }
      tokens.push({
        type: '量词',
        value: char,
        description: quantifierMap[char],
      })
      i++
      continue
    }

    // 花括号量词
    if (char === '{') {
      let end = i + 1
      while (end < pattern.length && pattern[end] !== '}') end++
      if (end < pattern.length) end++

      const quantifier = pattern.slice(i, end)
      tokens.push({
        type: '量词',
        value: quantifier,
        description: `匹配指定次数`,
      })
      i = end
      continue
    }

    // 位置锚点
    if (char === '^' || char === '$') {
      tokens.push({
        type: '锚点',
        value: char,
        description: char === '^' ? '匹配行首' : '匹配行尾',
      })
      i++
      continue
    }

    // 管道（交替）
    if (char === '|') {
      tokens.push({
        type: '交替',
        value: '|',
        description: '匹配左边或右边的表达式',
      })
      i++
      continue
    }

    // 字面量字符
    tokens.push({
      type: '字面量',
      value: char,
      description: `匹配字符 "${char}"`,
    })
    i++
  }

  return tokens
}

const highlightedParts = computed(() => {
  if (!testString.value || matches.value.length === 0) {
    return [{ text: testString.value, isMatch: false, isGroup: false }]
  }

  const parts: { text: string; isMatch: boolean; isGroup: boolean }[] = []
  let lastIndex = 0

  for (const match of matches.value) {
    const matchIndex = match.index ?? 0
    if (matchIndex > lastIndex) {
      parts.push({
        text: testString.value.slice(lastIndex, matchIndex),
        isMatch: false,
        isGroup: false,
      })
    }
    parts.push({
      text: match[0],
      isMatch: true,
      isGroup: false,
    })
    lastIndex = matchIndex + match[0].length
  }

  if (lastIndex < testString.value.length) {
    parts.push({
      text: testString.value.slice(lastIndex),
      isMatch: false,
      isGroup: false,
    })
  }

  return parts
})
</script>

<style scoped lang="scss">
.regex-visualizer-tool {
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

.regex-input-row {
  display: flex;
  gap: 8px;
}

.error-section {
  margin-top: 8px;
}

.tokens-tree {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.token-item {
  padding: 12px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
  border-left: 4px solid var(--n-primary-color);

  &.转义 {
    border-left-color: #52c41a;
  }

  &.字符类 {
    border-left-color: #1890ff;
  }

  &.捕获组,
  &.非捕获组,
  &.正向前瞻,
  &.负向前瞻,
  &.正向后顾,
  &.负向后顾 {
    border-left-color: #722ed1;
  }

  &.量词 {
    border-left-color: #fa8c16;
  }

  &.锚点 {
    border-left-color: #eb2f96;
  }

  &.交替 {
    border-left-color: #f5222d;
  }

  &.字面量 {
    border-left-color: #8c8c8c;
  }
}

.token-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 4px;
}

.token-type {
  font-weight: 500;
  color: var(--n-primary-color);
}

.token-value {
  font-family: monospace;
  background: var(--n-color);
  padding: 2px 6px;
  border-radius: 4px;
}

.token-description {
  color: var(--n-text-color-3);
  font-size: 12px;
}

.matches-visualization {
  margin-bottom: 16px;
}

.text-display {
  font-family: monospace;
  font-size: 16px;
  line-height: 1.5;
  padding: 12px;
  background: var(--n-color);
  border-radius: 4px;
  word-break: break-all;

  .match {
    background: var(--n-primary-color-suppl);
    color: var(--n-primary-color);
    padding: 2px 4px;
    border-radius: 2px;
  }

  .group {
    background: #52c41a33;
    color: #52c41a;
    padding: 2px 4px;
    border-radius: 2px;
  }
}

.matches-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.match-item {
  padding: 12px;
  border: 1px solid var(--n-border-color);
  border-radius: 4px;
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
</style>
