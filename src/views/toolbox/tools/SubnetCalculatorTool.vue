<template>
  <div class="subnet-calculator-tool">
    <div class="section">
      <div class="section-title">输入 IP 地址和子网掩码</div>
      <div class="input-row">
        <n-input
          v-model:value="ipAddress"
          placeholder="IP 地址（如 192.168.1.100）"
          @input="calculate"
        />
        <n-input
          v-model:value="subnetMask"
          placeholder="子网掩码（如 255.255.255.0 或 /24）"
          @input="calculate"
        />
      </div>
    </div>

    <div v-if="error" class="error-section">
      <n-alert type="error" :title="error" />
    </div>

    <div v-if="result" class="result-section">
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="网络地址">{{ result.networkAddress }}</n-descriptions-item>
        <n-descriptions-item label="广播地址">{{ result.broadcastAddress }}</n-descriptions-item>
        <n-descriptions-item label="子网掩码">{{ result.subnetMask }}</n-descriptions-item>
        <n-descriptions-item label="CIDR 表示">/{{ result.cidr }}</n-descriptions-item>
        <n-descriptions-item label="可用主机数">{{ result.usableHosts }}</n-descriptions-item>
        <n-descriptions-item label="IP 范围">{{ result.ipRange }}</n-descriptions-item>
        <n-descriptions-item label="第一个可用 IP">{{ result.firstUsable }}</n-descriptions-item>
        <n-descriptions-item label="最后一个可用 IP">{{ result.lastUsable }}</n-descriptions-item>
        <n-descriptions-item label="通配符掩码">{{ result.wildcardMask }}</n-descriptions-item>
        <n-descriptions-item label="IP 类型">{{ result.ipType }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">常用子网掩码速查</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>CIDR</th>
            <th>子网掩码</th>
            <th>可用主机数</th>
            <th>说明</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="item in commonSubnets" :key="item.cidr">
            <td>/{{ item.cidr }}</td>
            <td>{{ item.mask }}</td>
            <td>{{ item.hosts.toLocaleString() }}</td>
            <td>{{ item.description }}</td>
          </tr>
        </tbody>
      </n-table>
    </div>

    <div class="section">
      <div class="section-title">子网划分</div>
      <div class="input-row">
        <n-input-number
          v-model:value="subnetCount"
          placeholder="子网数量"
          :min="1"
          :max="1024"
          style="width: 120px"
        />
        <n-button type="primary" @click="divideSubnet">
          划分
        </n-button>
      </div>
      <div v-if="subnets.length > 0" class="subnets-list">
        <n-table :bordered="false" :single-line="false">
          <thead>
            <tr>
              <th>子网</th>
              <th>网络地址</th>
              <th>广播地址</th>
              <th>可用 IP 范围</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(subnet, index) in subnets" :key="index">
              <td>{{ index + 1 }}</td>
              <td>{{ subnet.network }}</td>
              <td>{{ subnet.broadcast }}</td>
              <td>{{ subnet.range }}</td>
            </tr>
          </tbody>
        </n-table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()
const ipAddress = ref('')
const subnetMask = ref('')
const error = ref('')
const result = ref<any>(null)
const subnetCount = ref(2)
const subnets = ref<any[]>([])

const commonSubnets = [
  { cidr: 8, mask: '255.0.0.0', hosts: 16777214, description: 'A 类默认' },
  { cidr: 16, mask: '255.255.0.0', hosts: 65534, description: 'B 类默认' },
  { cidr: 24, mask: '255.255.255.0', hosts: 254, description: 'C 类默认' },
  { cidr: 25, mask: '255.255.255.128', hosts: 126, description: '半个 C 类' },
  { cidr: 26, mask: '255.255.255.192', hosts: 62, description: '1/4 C 类' },
  { cidr: 27, mask: '255.255.255.224', hosts: 30, description: '1/8 C 类' },
  { cidr: 28, mask: '255.255.255.240', hosts: 14, description: '1/16 C 类' },
  { cidr: 29, mask: '255.255.255.248', hosts: 6, description: '1/32 C 类' },
  { cidr: 30, mask: '255.255.255.252', hosts: 2, description: '点对点链路' },
  { cidr: 32, mask: '255.255.255.255', hosts: 1, description: '单主机' },
]

function ipToNumber(ip: string): number {
  const parts = ip.split('.').map(Number)
  return ((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3]) >>> 0
}

function numberToIp(num: number): string {
  return [
    (num >>> 24) & 0xff,
    (num >>> 16) & 0xff,
    (num >>> 8) & 0xff,
    num & 0xff,
  ].join('.')
}

function cidrToMask(cidr: number): number {
  return cidr === 0 ? 0 : (~0 << (32 - cidr)) >>> 0
}

function maskToCidr(mask: number): number {
  let cidr = 0
  let m = mask
  while (m & 0x80000000) {
    cidr++
    m = (m << 1) >>> 0
  }
  return cidr
}

function validateIp(ip: string): boolean {
  const parts = ip.split('.')
  if (parts.length !== 4) return false
  return parts.every(part => {
    const num = parseInt(part, 10)
    return !isNaN(num) && num >= 0 && num <= 255
  })
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

function calculate() {
  error.value = ''
  result.value = null

  if (!ipAddress.value || !subnetMask.value) {
    return
  }

  if (!validateIp(ipAddress.value)) {
    error.value = 'IP 地址格式不正确'
    return
  }

  let mask: number
  const maskInput = subnetMask.value.trim()

  if (maskInput.startsWith('/')) {
    const cidr = parseInt(maskInput.substring(1), 10)
    if (isNaN(cidr) || cidr < 0 || cidr > 32) {
      error.value = 'CIDR 值必须在 0-32 之间'
      return
    }
    mask = cidrToMask(cidr)
  } else {
    if (!validateIp(maskInput)) {
      error.value = '子网掩码格式不正确'
      return
    }
    mask = ipToNumber(maskInput)
  }

  const ip = ipToNumber(ipAddress.value)
  const network = (ip & mask) >>> 0
  const broadcast = (network | ~mask) >>> 0
  const cidr = maskToCidr(mask)
  const usableHosts = Math.max(0, broadcast - network - 1)

  result.value = {
    networkAddress: numberToIp(network),
    broadcastAddress: numberToIp(broadcast),
    subnetMask: numberToIp(mask),
    cidr: cidr,
    usableHosts: usableHosts.toLocaleString(),
    ipRange: `${numberToIp(network + 1)} - ${numberToIp(broadcast - 1)}`,
    firstUsable: numberToIp(network + 1),
    lastUsable: numberToIp(broadcast - 1),
    wildcardMask: numberToIp(~mask >>> 0),
    ipType: getIpType(ipAddress.value),
  }
}

function divideSubnet() {
  if (!result.value) {
    message.warning('请先计算子网')
    return
  }

  const cidr = result.value.cidr
  const bitsNeeded = Math.ceil(Math.log2(subnetCount.value))
  const newCidr = cidr + bitsNeeded

  if (newCidr > 30) {
    message.error('子网划分超出范围')
    return
  }

  const network = ipToNumber(result.value.networkAddress)
  const subnetSize = 2 ** (32 - newCidr)
  const newSubnets = []

  for (let i = 0; i < subnetCount.value; i++) {
    const subnetNetwork = (network + i * subnetSize) >>> 0
    const subnetBroadcast = (subnetNetwork + subnetSize - 1) >>> 0
    newSubnets.push({
      network: numberToIp(subnetNetwork),
      broadcast: numberToIp(subnetBroadcast),
      range: `${numberToIp(subnetNetwork + 1)} - ${numberToIp(subnetBroadcast - 1)}`,
    })
  }

  subnets.value = newSubnets
}
</script>

<style scoped lang="scss">
.subnet-calculator-tool {
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
  align-items: center;
}

.error-section {
  margin-top: 8px;
}

.result-section {
  margin-top: 16px;
}

.subnets-list {
  margin-top: 12px;
}
</style>
