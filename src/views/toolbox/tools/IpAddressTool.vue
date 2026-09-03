<template>
  <div class="ip-address-tool">
    <div class="section">
      <div class="section-title">查询 IP 地址</div>
      <div class="input-row">
        <n-input
          v-model:value="ipAddress"
          placeholder="输入 IP 地址（留空查询本机）"
          @keyup.enter="queryIp"
        />
        <n-button type="primary" @click="queryIp" :loading="loading">
          查询
        </n-button>
      </div>
    </div>

    <div v-if="error" class="error-section">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="ipInfo" class="result-section">
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="IP 地址">{{ ipInfo.ip }}</n-descriptions-item>
        <n-descriptions-item label="类型">{{ ipInfo.type }}</n-descriptions-item>
        <n-descriptions-item label="国家">{{ ipInfo.country || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="地区">{{ ipInfo.region || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="城市">{{ ipInfo.city || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="ISP">{{ ipInfo.isp || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="组织">{{ ipInfo.org || '未知' }}</n-descriptions-item>
        <n-descriptions-item label="AS号">{{ ipInfo.as || '未知' }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">本机 IP 信息</div>
      <n-button @click="getLocalIp" :loading="localLoading">
        获取本机 IP
      </n-button>
      <div v-if="localIp" class="local-ip-info">
        <n-descriptions bordered :column="1">
          <n-descriptions-item label="公网 IP">{{ localIp }}</n-descriptions-item>
        </n-descriptions>
      </div>
    </div>

    <div class="section">
      <div class="section-title">IP 地址格式说明</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>类型</th>
            <th>范围</th>
            <th>说明</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>A 类</td>
            <td>1.0.0.0 - 126.255.255.255</td>
            <td>大型网络</td>
          </tr>
          <tr>
            <td>B 类</td>
            <td>128.0.0.0 - 191.255.255.255</td>
            <td>中型网络</td>
          </tr>
          <tr>
            <td>C 类</td>
            <td>192.0.0.0 - 223.255.255.255</td>
            <td>小型网络</td>
          </tr>
          <tr>
            <td>D 类</td>
            <td>224.0.0.0 - 239.255.255.255</td>
            <td>组播地址</td>
          </tr>
          <tr>
            <td>E 类</td>
            <td>240.0.0.0 - 255.255.255.255</td>
            <td>保留地址</td>
          </tr>
          <tr>
            <td>私有地址</td>
            <td>10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16</td>
            <td>局域网使用</td>
          </tr>
          <tr>
            <td>环回地址</td>
            <td>127.0.0.0/8</td>
            <td>本机回环</td>
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
const ipAddress = ref('')
const loading = ref(false)
const localLoading = ref(false)
const error = ref('')
const ipInfo = ref<any>(null)
const localIp = ref('')

async function queryIp() {
  if (!ipAddress.value) {
    // 查询本机公网 IP
    await getLocalIp()
    return
  }

  // 验证 IP 格式
  const ipRegex = /^(\d{1,3}\.){3}\d{1,3}$/
  if (!ipRegex.test(ipAddress.value)) {
    error.value = 'IP 地址格式不正确'
    return
  }

  loading.value = true
  error.value = ''
  ipInfo.value = null

  try {
    // 使用 ip-api.com 的免费 API
    const response = await fetch(`http://ip-api.com/json/${ipAddress.value}?lang=zh-CN`)
    const data = await response.json()

    if (data.status === 'success') {
      ipInfo.value = {
        ip: data.query,
        type: getIpType(data.query),
        country: data.country,
        region: data.regionName,
        city: data.city,
        isp: data.isp,
        org: data.org,
        as: data.as,
      }
    } else {
      error.value = '查询失败：' + data.message
    }
  } catch (e: any) {
    error.value = '查询失败：' + e.message
  } finally {
    loading.value = false
  }
}

async function getLocalIp() {
  localLoading.value = true
  try {
    const response = await fetch('https://api.ipify.org?format=json')
    const data = await response.json()
    localIp.value = data.ip
  } catch (e: any) {
    message.error('获取本机 IP 失败：' + e.message)
  } finally {
    localLoading.value = false
  }
}

function getIpType(ip: string): string {
  const parts = ip.split('.').map(Number)
  if (parts[0] === 127) return '环回地址'
  if (parts[0] === 10) return 'A 类私有地址'
  if (parts[0] === 172 && parts[1] >= 16 && parts[1] <= 31) return 'B 类私有地址'
  if (parts[0] === 192 && parts[1] === 168) return 'C 类私有地址'
  if (parts[0] >= 224 && parts[0] <= 239) return 'D 类组播地址'
  if (parts[0] >= 240) return 'E 类保留地址'
  return '公网地址'
}
</script>

<style scoped lang="scss">
.ip-address-tool {
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

.input-row {
  display: flex;
  gap: 8px;
}

.error-section {
  margin-top: 8px;
}

.result-section {
  margin-top: 16px;
}

.local-ip-info {
  margin-top: 12px;
}
</style>
