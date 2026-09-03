<template>
  <div class="ascii-art-tool">
    <div class="section">
      <div class="section-title">输入文本</div>
      <n-input
        v-model:value="inputText"
        placeholder="输入要转换的文本..."
        @input="generateArt"
      />
    </div>

    <div class="section">
      <div class="section-title">字体选择</div>
      <n-select v-model:value="selectedFont" :options="fontOptions" @update:value="generateArt" />
    </div>

    <div class="section">
      <div class="section-title">生成结果</div>
      <div class="art-output">
        <pre>{{ asciiArt }}</pre>
      </div>
      <n-button @click="copyToClipboard(asciiArt)" style="margin-top: 8px">
        复制
      </n-button>
    </div>

    <div class="section">
      <div class="section-title">ASCII 图案</div>
      <div class="patterns-grid">
        <div
          v-for="pattern in patterns"
          :key="pattern.name"
          class="pattern-item"
          @click="selectPattern(pattern)"
        >
          <div class="pattern-name">{{ pattern.name }}</div>
          <pre class="pattern-preview">{{ pattern.art }}</pre>
        </div>
      </div>
    </div>

    <div class="section">
      <div class="section-title">文本转 ASCII 数字</div>
      <n-input
        v-model:value="textToConvert"
        placeholder="输入文本..."
        @input="convertToAscii"
      />
      <div v-if="asciiNumbers" class="ascii-numbers">
        <n-code :code="asciiNumbers" language="text" />
        <n-button size="small" @click="copyToClipboard(asciiNumbers)" style="margin-top: 8px">
          复制
        </n-button>
      </div>
    </div>

    <div class="section">
      <div class="section-title">ASCII 表</div>
      <n-table :bordered="false" :single-line="false" size="small">
        <thead>
          <tr>
            <th>Dec</th>
            <th>Hex</th>
            <th>Char</th>
            <th>Dec</th>
            <th>Hex</th>
            <th>Char</th>
            <th>Dec</th>
            <th>Hex</th>
            <th>Char</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in asciiTableRows" :key="row[0].dec">
            <template v-for="(cell, idx) in row" :key="idx">
              <td>{{ cell.dec }}</td>
              <td>{{ cell.hex }}</td>
              <td><n-code :code="cell.char" language="text" /></td>
            </template>
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
const inputText = ref('')
const asciiArt = ref('')
const selectedFont = ref('standard')
const textToConvert = ref('')
const asciiNumbers = ref('')

const fontOptions = [
  { label: '标准', value: 'standard' },
  { label: '粗体', value: 'bold' },
  { label: '等宽', value: 'mono' },
]

const patterns = [
  {
    name: '笑脸',
    art: `  ****
 *    *
* *  * *
*      *
* *  * *
*  **  *
 *    *
  ****`,
  },
  {
    name: '心形',
    art: `  **  **
 *  **  *
*        *
*        *
 *      *
  *    *
   *  *
    *`,
  },
  {
    name: '星星',
    art: `    *
   ***
*********
   ***
  * * *
 *   *`,
  },
  {
    name: '箭头',
    art: `    *
   ***
  *****
 *******
*********
   ***
   ***
   ***`,
  },
  {
    name: '猫',
    art: ` /\\_/\\
( o.o )
 > ^ <
/|   |\\
(_|_|_)`,
  },
  {
    name: '房子',
    art: `    /\\
   /  \\
  /    \\
 /______\\
 |      |
 |  []  |
 |______|`,
  },
]

const asciiChars = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@']

function generateArt() {
  if (!inputText.value) {
    asciiArt.value = ''
    return
  }

  // 简单的 ASCII 艺术生成
  const text = inputText.value.toUpperCase()
  const lines: string[] = []

  for (let i = 0; i < 7; i++) {
    let line = ''
    for (const char of text) {
      line += getCharLine(char, i) + ' '
    }
    lines.push(line)
  }

  asciiArt.value = lines.join('\n')
}

function getCharLine(char: string, line: number): string {
  const chars: Record<string, string[]> = {
    'A': ['  #  ', ' # # ', '#   #', '#####', '#   #', '#   #', '#   #'],
    'B': ['#### ', '#   #', '#   #', '#### ', '#   #', '#   #', '#### '],
    'C': [' ####', '#    ', '#    ', '#    ', '#    ', '#    ', ' ####'],
    'D': ['#### ', '#   #', '#   #', '#   #', '#   #', '#   #', '#### '],
    'E': ['#####', '#    ', '#    ', '#### ', '#    ', '#    ', '#####'],
    'F': ['#####', '#    ', '#    ', '#### ', '#    ', '#    ', '#    '],
    'G': [' ####', '#    ', '#    ', '# ###', '#   #', '#   #', ' ####'],
    'H': ['#   #', '#   #', '#   #', '#####', '#   #', '#   #', '#   #'],
    'I': ['#####', '  #  ', '  #  ', '  #  ', '  #  ', '  #  ', '#####'],
    'J': ['#####', '    #', '    #', '    #', '    #', '#   #', ' ### '],
    'K': ['#   #', '#  # ', '# #  ', '##   ', '# #  ', '#  # ', '#   #'],
    'L': ['#    ', '#    ', '#    ', '#    ', '#    ', '#    ', '#####'],
    'M': ['#   #', '## ##', '# # #', '#   #', '#   #', '#   #', '#   #'],
    'N': ['#   #', '##  #', '# # #', '#  ##', '#   #', '#   #', '#   #'],
    'O': [' ### ', '#   #', '#   #', '#   #', '#   #', '#   #', ' ### '],
    'P': ['#### ', '#   #', '#   #', '#### ', '#    ', '#    ', '#    '],
    'Q': [' ### ', '#   #', '#   #', '#   #', '# # #', '#  # ', ' ## #'],
    'R': ['#### ', '#   #', '#   #', '#### ', '# #  ', '#  # ', '#   #'],
    'S': [' ####', '#    ', '#    ', ' ### ', '    #', '    #', '#### '],
    'T': ['#####', '  #  ', '  #  ', '  #  ', '  #  ', '  #  ', '  #  '],
    'U': ['#   #', '#   #', '#   #', '#   #', '#   #', '#   #', ' ### '],
    'V': ['#   #', '#   #', '#   #', '#   #', ' # # ', ' # # ', '  #  '],
    'W': ['#   #', '#   #', '#   #', '# # #', '# # #', '## ##', '#   #'],
    'X': ['#   #', '#   #', ' # # ', '  #  ', ' # # ', '#   #', '#   #'],
    'Y': ['#   #', '#   #', ' # # ', '  #  ', '  #  ', '  #  ', '  #  '],
    'Z': ['#####', '    #', '   # ', '  #  ', ' #   ', '#    ', '#####'],
    '0': [' ### ', '#   #', '#  ##', '# # #', '##  #', '#   #', ' ### '],
    '1': ['  #  ', ' ##  ', '  #  ', '  #  ', '  #  ', '  #  ', '#####'],
    '2': [' ### ', '#   #', '    #', ' ### ', '#    ', '#    ', '#####'],
    '3': [' ### ', '#   #', '    #', ' ### ', '    #', '#   #', ' ### '],
    '4': ['   # ', '  ## ', ' # # ', '#  # ', '#####', '   # ', '   # '],
    '5': ['#####', '#    ', '#    ', '#### ', '    #', '#   #', ' ### '],
    '6': [' ### ', '#    ', '#    ', '#### ', '#   #', '#   #', ' ### '],
    '7': ['#####', '    #', '   # ', '  #  ', '  #  ', '  #  ', '  #  '],
    '8': [' ### ', '#   #', '#   #', ' ### ', '#   #', '#   #', ' ### '],
    '9': [' ### ', '#   #', '#   #', ' ####', '    #', '    #', ' ### '],
    ' ': ['     ', '     ', '     ', '     ', '     ', '     ', '     '],
  }

  return chars[char]?.[line] || '     '
}

function selectPattern(pattern: any) {
  asciiArt.value = pattern.art
}

function convertToAscii() {
  if (!textToConvert.value) {
    asciiNumbers.value = ''
    return
  }
  const codes = Array.from(textToConvert.value).map(c => c.charCodeAt(0))
  asciiNumbers.value = codes.join(' ')
}

const asciiTableRows = computed(() => {
  const rows: { dec: number; hex: string; char: string }[][] = []
  for (let i = 32; i < 127; i += 3) {
    const row = []
    for (let j = 0; j < 3 && i + j < 127; j++) {
      const dec = i + j
      row.push({
        dec,
        hex: dec.toString(16).toUpperCase().padStart(2, '0'),
        char: String.fromCharCode(dec),
      })
    }
    rows.push(row)
  }
  return rows
})

function copyToClipboard(text: string) {
  if (!text) return
  navigator.clipboard.writeText(text).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.ascii-art-tool {
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

.art-output {
  background: var(--n-color);
  padding: 16px;
  border-radius: 4px;
  overflow-x: auto;

  pre {
    font-family: monospace;
    font-size: 14px;
    line-height: 1.2;
    margin: 0;
  }
}

.patterns-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}

.pattern-item {
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

.pattern-name {
  font-weight: 500;
  margin-bottom: 8px;
}

.pattern-preview {
  font-family: monospace;
  font-size: 12px;
  line-height: 1.2;
  margin: 0;
  white-space: pre;
}

.ascii-numbers {
  margin-top: 8px;
  padding: 12px;
  background: var(--n-color);
  border-radius: 4px;
}
</style>
