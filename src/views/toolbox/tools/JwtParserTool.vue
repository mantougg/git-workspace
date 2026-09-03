<template>
  <div class="jwt-parser-tool">
    <div class="tool-header">
      <n-input
        v-model:value="jwtInput"
        type="textarea"
        placeholder="粘贴 JWT Token..."
        :rows="4"
        @input="parseJwt"
      />
      <n-button type="primary" @click="parseJwt" :disabled="!jwtInput">
        解析
      </n-button>
    </div>

    <div v-if="error" class="error-message">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="parsed" class="jwt-parts">
      <n-tabs type="segment" animated>
        <n-tab-pane name="header" tab="Header">
          <div class="part-header">
            <span class="part-label">Header</span>
            <n-button size="small" @click="copyToClipboard(headerJson)">
              复制
            </n-button>
          </div>
          <n-code :code="headerJson" language="json" />
        </n-tab-pane>

        <n-tab-pane name="payload" tab="Payload">
          <div class="part-header">
            <span class="part-label">Payload</span>
            <n-button size="small" @click="copyToClipboard(payloadJson)">
              复制
            </n-button>
          </div>
          <n-code :code="payloadJson" language="json" />
        </n-tab-pane>

        <n-tab-pane name="signature" tab="Signature">
          <div class="part-header">
            <span class="part-label">Signature</span>
          </div>
          <n-input
            :value="signature"
            type="textarea"
            readonly
            :rows="3"
          />
        </n-tab-pane>
      </n-tabs>

      <div v-if="payload.exp || payload.iat || payload.nbf" class="time-info">
        <n-descriptions bordered :column="1">
          <n-descriptions-item v-if="payload.iat" label="签发时间 (iat)">
            {{ formatTimestamp(payload.iat) }}
          </n-descriptions-item>
          <n-descriptions-item v-if="payload.exp" label="过期时间 (exp)">
            {{ formatTimestamp(payload.exp) }}
            <n-tag :type="isExpired ? 'error' : 'success'" size="small">
              {{ isExpired ? '已过期' : '有效' }}
            </n-tag>
          </n-descriptions-item>
          <n-descriptions-item v-if="payload.nbf" label="生效时间 (nbf)">
            {{ formatTimestamp(payload.nbf) }}
          </n-descriptions-item>
        </n-descriptions>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const jwtInput = ref('')
const parsed = ref(false)
const error = ref('')
const header = ref<any>({})
const payload = ref<any>({})
const signature = ref('')

const headerJson = computed(() => JSON.stringify(header.value, null, 2))
const payloadJson = computed(() => JSON.stringify(payload.value, null, 2))

const isExpired = computed(() => {
  if (!payload.value.exp) return false
  return Date.now() >= payload.value.exp * 1000
})

function base64UrlDecode(str: string): string {
  let base64 = str.replace(/-/g, '+').replace(/_/g, '/')
  const pad = base64.length % 4
  if (pad) {
    base64 += '='.repeat(4 - pad)
  }
  return atob(base64)
}

function parseJwt() {
  error.value = ''
  parsed.value = false

  if (!jwtInput.value.trim()) {
    return
  }

  const parts = jwtInput.value.trim().split('.')
  if (parts.length !== 3) {
    error.value = '无效的 JWT 格式：应包含三部分（header.payload.signature）'
    return
  }

  try {
    const headerJson = base64UrlDecode(parts[0])
    header.value = JSON.parse(headerJson)
  } catch (e) {
    error.value = 'Header 解析失败：无效的 Base64 或 JSON'
    return
  }

  try {
    const payloadJson = base64UrlDecode(parts[1])
    payload.value = JSON.parse(payloadJson)
  } catch (e) {
    error.value = 'Payload 解析失败：无效的 Base64 或 JSON'
    return
  }

  signature.value = parts[2]
  parsed.value = true
}

function formatTimestamp(timestamp: number): string {
  const date = new Date(timestamp * 1000)
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
.jwt-parser-tool {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.tool-header {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.error-message {
  margin-top: 8px;
}

.jwt-parts {
  margin-top: 16px;
}

.part-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}

.part-label {
  font-weight: 500;
  color: var(--n-text-color);
}

.time-info {
  margin-top: 16px;
}
</style>
