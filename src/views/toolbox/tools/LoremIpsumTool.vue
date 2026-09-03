<template>
  <div class="lorem-ipsum-tool">
    <div class="section">
      <div class="section-title">生成选项</div>
      <div class="options-row">
        <div class="option">
          <label>类型</label>
          <n-radio-group v-model:value="generateType">
            <n-space>
              <n-radio value="paragraphs">段落</n-radio>
              <n-radio value="sentences">句子</n-radio>
              <n-radio value="words">单词</n-radio>
            </n-space>
          </n-radio-group>
        </div>
        <div class="option">
          <label>数量</label>
          <n-input-number v-model:value="count" :min="1" :max="100" style="width: 100px" />
        </div>
        <div class="option">
          <label>语言</label>
          <n-select v-model:value="language" :options="languageOptions" style="width: 120px" />
        </div>
      </div>
      <n-button type="primary" @click="generate" style="margin-top: 12px">
        生成
      </n-button>
    </div>

    <div class="section">
      <div class="section-title">生成结果</div>
      <n-input
        v-model:value="generatedText"
        type="textarea"
        readonly
        :rows="10"
        placeholder="点击生成按钮..."
      />
      <div class="result-actions">
        <n-button @click="copyToClipboard(generatedText)">
          复制文本
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
          <div class="template-preview">{{ template.preview }}</div>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">统计信息</div>
      <n-descriptions bordered :column="3">
        <n-descriptions-item label="字符数">{{ stats.characters }}</n-descriptions-item>
        <n-descriptions-item label="单词数">{{ stats.words }}</n-descriptions-item>
        <n-descriptions-item label="段落数">{{ stats.paragraphs }}</n-descriptions-item>
      </n-descriptions>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const generateType = ref('paragraphs')
const count = ref(3)
const language = ref('latin')
const generatedText = ref('')

const languageOptions = [
  { label: '拉丁文', value: 'latin' },
  { label: '中文', value: 'chinese' },
  { label: '英文', value: 'english' },
]

const templates = [
  {
    name: '标准 Lorem Ipsum',
    preview: 'Lorem ipsum dolor sit amet...',
    type: 'paragraphs',
    count: 3,
    language: 'latin',
  },
  {
    name: '中文乱数假文',
    preview: '天地玄黄宇宙洪荒...',
    type: 'paragraphs',
    count: 3,
    language: 'chinese',
  },
  {
    name: '英文测试文本',
    preview: 'The quick brown fox...',
    type: 'paragraphs',
    count: 3,
    language: 'english',
  },
]

const loremLatin = [
  'Lorem ipsum dolor sit amet, consectetur adipiscing elit.',
  'Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.',
  'Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris.',
  'Nisi ut aliquip ex ea commodo consequat.',
  'Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore.',
  'Eu fugiat nulla pariatur.',
  'Excepteur sint occaecat cupidatat non proident.',
  'Sunt in culpa qui officia deserunt mollit anim id est laborum.',
  'Curabitur pretium tincidunt lacus.',
  'Nulla gravida orci a odio.',
  'Nullam varius, turpis et commodo pharetra.',
  'Est eros bibendum elit, nec luctus magna felis sollicitudin mauris.',
  'Integer in mauris eu nibh euismod gravida.',
  'Duis ac tellus et risus vulputate vehicula.',
  'Donec lobortis risus a elit.',
  'Etiam tempor.',
  'Ut ullamcorper, ligula ut dictum pharetra, nisi nunc fringilla magna.',
  'In commodo nisl nec velit.',
  'Maecenas aliquet augue vel augue.',
  'Nam elit agna, endrerit sit amet, tincidunt ac, viverra sed, nulla.',
]

const loremChinese = [
  '天地玄黄，宇宙洪荒。日月盈昃，辰宿列张。',
  '寒来暑往，秋收冬藏。闰余成岁，律吕调阳。',
  '云腾致雨，露结为霜。金生丽水，玉出昆冈。',
  '剑号巨阙，珠称夜光。果珍李柰，菜重芥姜。',
  '海咸河淡，鳞潜羽翔。龙师火帝，鸟官人皇。',
  '始制文字，乃服衣裳。推位让国，有虞陶唐。',
  '吊民伐罪，周发殷汤。坐朝问道，垂拱平章。',
  '爱育黎首，臣伏戎羌。遐迩一体，率宾归王。',
  '鸣凤在竹，白驹食场。化被草木，赖及万方。',
  '盖此身发，四大五常。恭惟鞠养，岂敢毁伤。',
  '女慕贞洁，男效才良。知过必改，得能莫忘。',
  '罔谈彼短，靡恃己长。信使可覆，器欲难量。',
  '墨悲丝染，诗赞羔羊。景行维贤，克念作圣。',
  '德建名立，形端表正。空谷传声，虚堂习听。',
  '祸因恶积，福缘善庆。尺璧非宝，寸阴是竞。',
]

const loremEnglish = [
  'The quick brown fox jumps over the lazy dog.',
  'Pack my box with five dozen liquor jugs.',
  'How vexingly quick daft zebras jump.',
  'The five boxing wizards jump quickly.',
  'Jackdaws love my big sphinx of quartz.',
  'Mr. Jock, TV quiz PhD, bags few lynx.',
  'Crazy Frederick bought many very exquisite opal jewels.',
  'We promptly judged antique ivory buckles for the next prize.',
  'A mad boxer shot a quick, gloved jab to the jaw of his dizzy opponent.',
  'Jaded zombies acted quaintly but kept driving their oxen forward.',
  'The job requires extra pluck and zeal from every young wage earner.',
  'Just work for improved basic techniques to maximize your typing skill.',
  'The wizard quickly jinxed the gnomes before they vaporized.',
  'Few black taxis drive up major roads on quiet hazy nights.',
  'With tenure, Suzie’d have a cushy job.',
  'However, she chose to pursue her passion for art instead.',
  'The morning sun cast long shadows across the dewy meadow.',
  'Birds sang their familiar melodies as the day began.',
  'A gentle breeze carried the scent of wildflowers through the air.',
  'The old oak tree stood majestically at the edge of the forest.',
]

function generate() {
  let result = ''

  switch (generateType.value) {
    case 'paragraphs':
      result = generateParagraphs(count.value)
      break
    case 'sentences':
      result = generateSentences(count.value)
      break
    case 'words':
      result = generateWords(count.value)
      break
  }

  generatedText.value = result
}

function generateParagraphs(count: number): string {
  const paragraphs: string[] = []
  for (let i = 0; i < count; i++) {
    const sentences = generateSentences(randomInt(3, 6))
    paragraphs.push(sentences)
  }
  return paragraphs.join('\n\n')
}

function generateSentences(count: number): string {
  const sentences: string[] = []
  const wordList = getWordList()

  for (let i = 0; i < count; i++) {
    const sentenceLength = randomInt(5, 15)
    const words: string[] = []
    for (let j = 0; j < sentenceLength; j++) {
      words.push(wordList[randomInt(0, wordList.length - 1)])
    }
    let sentence = words.join(' ')
    sentence = sentence.charAt(0).toUpperCase() + sentence.slice(1) + '.'
    sentences.push(sentence)
  }

  return sentences.join(' ')
}

function generateWords(count: number): string {
  const wordList = getWordList()
  const words: string[] = []
  for (let i = 0; i < count; i++) {
    words.push(wordList[randomInt(0, wordList.length - 1)])
  }
  return words.join(' ')
}

function getWordList(): string[] {
  switch (language.value) {
    case 'latin':
      return loremLatin.flatMap(s => s.replace(/[.,]/g, '').split(' '))
    case 'chinese':
      return loremChinese.flatMap(s => s.replace(/[，。]/g, '').split(''))
    case 'english':
      return loremEnglish.flatMap(s => s.replace(/[.,]/g, '').split(' '))
    default:
      return loremLatin.flatMap(s => s.replace(/[.,]/g, '').split(' '))
  }
}

function randomInt(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min
}

function applyTemplate(template: any) {
  generateType.value = template.type
  count.value = template.count
  language.value = template.language
  generate()
}

function clear() {
  generatedText.value = ''
}

const stats = computed(() => {
  const text = generatedText.value
  return {
    characters: text.length,
    words: text ? text.split(/\s+/).filter(w => w.length > 0).length : 0,
    paragraphs: text ? text.split(/\n\n/).filter(p => p.trim().length > 0).length : 0,
  }
})

function copyToClipboard(text: string) {
  if (!text) return
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.lorem-ipsum-tool {
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

.template-preview {
  font-size: 12px;
  color: var(--n-text-color-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
