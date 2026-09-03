<template>
  <div class="text-stats-tool">
    <div class="section">
      <div class="section-title">输入文本</div>
      <n-input
        v-model:value="inputText"
        type="textarea"
        placeholder="输入要分析的文本..."
        :rows="8"
      />
    </div>

    <div class="section">
      <div class="section-title">基本统计</div>
      <n-descriptions bordered :column="3">
        <n-descriptions-item label="字符数">{{ stats.characters }}</n-descriptions-item>
        <n-descriptions-item label="字符数（不含空格）">{{ stats.charactersNoSpaces }}</n-descriptions-item>
        <n-descriptions-item label="单词数">{{ stats.words }}</n-descriptions-item>
        <n-descriptions-item label="行数">{{ stats.lines }}</n-descriptions-item>
        <n-descriptions-item label="段落数">{{ stats.paragraphs }}</n-descriptions-item>
        <n-descriptions-item label="句子数">{{ stats.sentences }}</n-descriptions-item>
        <n-descriptions-item label="中文字符数">{{ stats.chineseChars }}</n-descriptions-item>
        <n-descriptions-item label="英文单词数">{{ stats.englishWords }}</n-descriptions-item>
        <n-descriptions-item label="数字个数">{{ stats.numbers }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">可读性分析</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="平均单词长度">{{ readability.avgWordLength }} 字符</n-descriptions-item>
        <n-descriptions-item label="平均句子长度">{{ readability.avgSentenceLength }} 单词</n-descriptions-item>
        <n-descriptions-item label="阅读时间（中文）">{{ readability.readingTimeChinese }}</n-descriptions-item>
        <n-descriptions-item label="阅读时间（英文）">{{ readability.readingTimeEnglish }}</n-descriptions-item>
        <n-descriptions-item label="说话时间">{{ readability.speakingTime }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">字符频率（Top 10）</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>字符</th>
            <th>次数</th>
            <th>占比</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in charFrequency" :key="item.char">
            <td><n-code :code="item.char" language="text" /></td>
            <td>{{ item.count }}</td>
            <td>{{ item.percentage }}%</td>
          </tr>
        </tbody>
      </n-table>
    </div>

    <div class="section">
      <div class="section-title">单词频率（Top 10）</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>单词</th>
            <th>次数</th>
            <th>占比</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in wordFrequency" :key="item.word">
            <td>{{ item.word }}</td>
            <td>{{ item.count }}</td>
            <td>{{ item.percentage }}%</td>
          </tr>
        </tbody>
      </n-table>
    </div>

    <div class="section">
      <div class="section-title">字符类型分布</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="字母">{{ charTypes.letters }} ({{ charTypes.lettersPercent }}%)</n-descriptions-item>
        <n-descriptions-item label="数字">{{ charTypes.digits }} ({{ charTypes.digitsPercent }}%)</n-descriptions-item>
        <n-descriptions-item label="空格">{{ charTypes.spaces }} ({{ charTypes.spacesPercent }}%)</n-descriptions-item>
        <n-descriptions-item label="标点符号">{{ charTypes.punctuation }} ({{ charTypes.punctuationPercent }}%)</n-descriptions-item>
        <n-descriptions-item label="其他">{{ charTypes.others }} ({{ charTypes.othersPercent }}%)</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <n-button type="primary" @click="copyStats">
        复制统计结果
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const inputText = ref('')

const stats = computed(() => {
  const text = inputText.value
  if (!text) {
    return {
      characters: 0,
      charactersNoSpaces: 0,
      words: 0,
      lines: 0,
      paragraphs: 0,
      sentences: 0,
      chineseChars: 0,
      englishWords: 0,
      numbers: 0,
    }
  }

  const characters = text.length
  const charactersNoSpaces = text.replace(/\s/g, '').length
  const words = text.trim() ? text.trim().split(/\s+/).length : 0
  const lines = text.split('\n').length
  const paragraphs = text.split(/\n\s*\n/).filter(p => p.trim()).length || (text.trim() ? 1 : 0)
  const sentences = text.split(/[.!?。！？]+/).filter(s => s.trim()).length
  const chineseChars = (text.match(/[\u4e00-\u9fa5]/g) || []).length
  const englishWords = text.match(/[a-zA-Z]+/g)?.length || 0
  const numbers = (text.match(/\d+/g) || []).length

  return {
    characters,
    charactersNoSpaces,
    words,
    lines,
    paragraphs,
    sentences,
    chineseChars,
    englishWords,
    numbers,
  }
})

const readability = computed(() => {
  const text = inputText.value
  if (!text) {
    return {
      avgWordLength: 0,
      avgSentenceLength: 0,
      readingTimeChinese: '0 分钟',
      readingTimeEnglish: '0 分钟',
      speakingTime: '0 分钟',
    }
  }

  const words = text.trim().split(/\s+/)
  const avgWordLength = words.length > 0
    ? (words.reduce((sum, w) => sum + w.length, 0) / words.length).toFixed(1)
    : 0

  const sentences = text.split(/[.!?。！？]+/).filter(s => s.trim())
  const avgSentenceLength = sentences.length > 0
    ? (words.length / sentences.length).toFixed(1)
    : 0

  // 中文阅读速度：约500字/分钟
  const chineseChars = (text.match(/[\u4e00-\u9fa5]/g) || []).length
  const readingTimeChinese = Math.ceil(chineseChars / 500)

  // 英文阅读速度：约200词/分钟
  const englishWords = text.match(/[a-zA-Z]+/g)?.length || 0
  const readingTimeEnglish = Math.ceil(englishWords / 200)

  // 说话速度：约150词/分钟
  const speakingTime = Math.ceil((chineseChars / 300 + englishWords / 150))

  return {
    avgWordLength,
    avgSentenceLength,
    readingTimeChinese: `${readingTimeChinese} 分钟`,
    readingTimeEnglish: `${readingTimeEnglish} 分钟`,
    speakingTime: `${speakingTime} 分钟`,
  }
})

const charFrequency = computed(() => {
  const text = inputText.value
  if (!text) return []

  const freq: Record<string, number> = {}
  for (const char of text) {
    if (char.trim()) {
      freq[char] = (freq[char] || 0) + 1
    }
  }

  return Object.entries(freq)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([char, count]) => ({
      char,
      count,
      percentage: ((count / text.length) * 100).toFixed(1),
    }))
})

const wordFrequency = computed(() => {
  const text = inputText.value
  if (!text) return []

  const words = text.toLowerCase().match(/[a-zA-Z]+|[\u4e00-\u9fa5]/g) || []
  const freq: Record<string, number> = {}
  for (const word of words) {
    if (word.length > 1) {
      freq[word] = (freq[word] || 0) + 1
    }
  }

  return Object.entries(freq)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([word, count]) => ({
      word,
      count,
      percentage: ((count / words.length) * 100).toFixed(1),
    }))
})

const charTypes = computed(() => {
  const text = inputText.value
  if (!text) {
    return {
      letters: 0, lettersPercent: 0,
      digits: 0, digitsPercent: 0,
      spaces: 0, spacesPercent: 0,
      punctuation: 0, punctuationPercent: 0,
      others: 0, othersPercent: 0,
    }
  }

  let letters = 0, digits = 0, spaces = 0, punctuation = 0, others = 0
  for (const char of text) {
    if (/[a-zA-Z\u4e00-\u9fa5]/.test(char)) letters++
    else if (/\d/.test(char)) digits++
    else if (/\s/.test(char)) spaces++
    else if (/[.,;:!?'"(){}[\]<>@#$%^&*+=_\-~`|\\/]/.test(char)) punctuation++
    else others++
  }

  const total = text.length
  return {
    letters,
    lettersPercent: ((letters / total) * 100).toFixed(1),
    digits,
    digitsPercent: ((digits / total) * 100).toFixed(1),
    spaces,
    spacesPercent: ((spaces / total) * 100).toFixed(1),
    punctuation,
    punctuationPercent: ((punctuation / total) * 100).toFixed(1),
    others,
    othersPercent: ((others / total) * 100).toFixed(1),
  }
})

function copyStats() {
  const text = `文本统计结果：
字符数：${stats.value.characters}
字符数（不含空格）：${stats.value.charactersNoSpaces}
单词数：${stats.value.words}
行数：${stats.value.lines}
段落数：${stats.value.paragraphs}
句子数：${stats.value.sentences}
中文字符数：${stats.value.chineseChars}
英文单词数：${stats.value.englishWords}
数字个数：${stats.value.numbers}
平均单词长度：${readability.value.avgWordLength} 字符
平均句子长度：${readability.value.avgSentenceLength} 单词
阅读时间（中文）：${readability.value.readingTimeChinese}
阅读时间（英文）：${readability.value.readingTimeEnglish}
说话时间：${readability.value.speakingTime}`

  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.text-stats-tool {
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
</style>
