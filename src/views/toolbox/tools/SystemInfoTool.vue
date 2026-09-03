<template>
  <div class="system-info-tool">
    <div class="section">
      <div class="section-title">浏览器信息</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="浏览器">{{ browserInfo.name }}</n-descriptions-item>
        <n-descriptions-item label="版本">{{ browserInfo.version }}</n-descriptions-item>
        <n-descriptions-item label="引擎">{{ browserInfo.engine }}</n-descriptions-item>
        <n-descriptions-item label="用户代理">{{ browserInfo.userAgent }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">操作系统</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="平台">{{ osInfo.platform }}</n-descriptions-item>
        <n-descriptions-item label="语言">{{ osInfo.language }}</n-descriptions-item>
        <n-descriptions-item label="屏幕分辨率">{{ osInfo.screenResolution }}</n-descriptions-item>
        <n-descriptions-item label="像素比">{{ osInfo.pixelRatio }}</n-descriptions-item>
        <n-descriptions-item label="时区">{{ osInfo.timezone }}</n-descriptions-item>
        <n-descriptions-item label="在线状态">{{ osInfo.onLine ? '在线' : '离线' }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">硬件信息</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="CPU 核心数">{{ hardwareInfo.cpuCores }}</n-descriptions-item>
        <n-descriptions-item label="内存">{{ hardwareInfo.memory }}</n-descriptions-item>
        <n-descriptions-item label="最大触点数">{{ hardwareInfo.maxTouchPoints }}</n-descriptions-item>
        <n-descriptions-item label="设备类型">{{ hardwareInfo.deviceType }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">网络信息</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="连接类型">{{ networkInfo.effectiveType }}</n-descriptions-item>
        <n-descriptions-item label="下行速度">{{ networkInfo.downlink }}</n-descriptions-item>
        <n-descriptions-item label="往返时间">{{ networkInfo.rtt }}</n-descriptions-item>
        <n-descriptions-item label="节省数据">{{ networkInfo.saveData ? '是' : '否' }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">显示信息</div>
      <n-descriptions bordered :column="2">
        <n-descriptions-item label="窗口大小">{{ displayInfo.windowSize }}</n-descriptions-item>
        <n-descriptions-item label="文档大小">{{ displayInfo.documentSize }}</n-descriptions-item>
        <n-descriptions-item label="颜色深度">{{ displayInfo.colorDepth }}</n-descriptions-item>
        <n-descriptions-item label="方向">{{ displayInfo.orientation }}</n-descriptions-item>
      </n-descriptions>
    </div>

    <div class="section">
      <div class="section-title">功能支持</div>
      <n-table :bordered="false" :single-line="false">
        <thead>
          <tr>
            <th>功能</th>
            <th>支持状态</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(value, key) in featureSupport" :key="key">
            <td>{{ key }}</td>
            <td>
              <n-tag :type="value ? 'success' : 'error'" size="small">
                {{ value ? '支持' : '不支持' }}
              </n-tag>
            </td>
          </tr>
        </tbody>
      </n-table>
    </div>

    <div class="section">
      <n-button type="primary" @click="copyAllInfo">
        复制全部信息
      </n-button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useMessage } from 'naive-ui'

const message = useMessage()

const browserInfo = ref({
  name: '',
  version: '',
  engine: '',
  userAgent: '',
})

const osInfo = ref({
  platform: '',
  language: '',
  screenResolution: '',
  pixelRatio: '',
  timezone: '',
  onLine: true,
})

const hardwareInfo = ref({
  cpuCores: '',
  memory: '',
  maxTouchPoints: '',
  deviceType: '',
})

const networkInfo = ref({
  effectiveType: '',
  downlink: '',
  rtt: '',
  saveData: false,
})

const displayInfo = ref({
  windowSize: '',
  documentSize: '',
  colorDepth: '',
  orientation: '',
})

const featureSupport = ref<Record<string, boolean>>({})

onMounted(() => {
  collectBrowserInfo()
  collectOsInfo()
  collectHardwareInfo()
  collectNetworkInfo()
  collectDisplayInfo()
  checkFeatureSupport()
})

function collectBrowserInfo() {
  const ua = navigator.userAgent
  browserInfo.value.userAgent = ua

  // 检测浏览器
  if (ua.includes('Firefox')) {
    browserInfo.value.name = 'Firefox'
    browserInfo.value.engine = 'Gecko'
    const match = ua.match(/Firefox\/(\d+)/)
    browserInfo.value.version = match ? match[1] : '未知'
  } else if (ua.includes('Edg')) {
    browserInfo.value.name = 'Edge'
    browserInfo.value.engine = 'Blink'
    const match = ua.match(/Edg\/(\d+)/)
    browserInfo.value.version = match ? match[1] : '未知'
  } else if (ua.includes('Chrome')) {
    browserInfo.value.name = 'Chrome'
    browserInfo.value.engine = 'Blink'
    const match = ua.match(/Chrome\/(\d+)/)
    browserInfo.value.version = match ? match[1] : '未知'
  } else if (ua.includes('Safari')) {
    browserInfo.value.name = 'Safari'
    browserInfo.value.engine = 'WebKit'
    const match = ua.match(/Version\/(\d+)/)
    browserInfo.value.version = match ? match[1] : '未知'
  } else {
    browserInfo.value.name = '未知'
    browserInfo.value.version = '未知'
    browserInfo.value.engine = '未知'
  }
}

function collectOsInfo() {
  osInfo.value.platform = navigator.platform
  osInfo.value.language = navigator.language
  osInfo.value.screenResolution = `${screen.width} × ${screen.height}`
  osInfo.value.pixelRatio = `${window.devicePixelRatio}x`
  osInfo.value.timezone = Intl.DateTimeFormat().resolvedOptions().timeZone
  osInfo.value.onLine = navigator.onLine
}

function collectHardwareInfo() {
  hardwareInfo.value.cpuCores = `${navigator.hardwareConcurrency || '未知'} 核`

  // @ts-ignore
  const memory = navigator.deviceMemory
  hardwareInfo.value.memory = memory ? `${memory} GB` : '未知'

  hardwareInfo.value.maxTouchPoints = `${navigator.maxTouchPoints}`

  // 检测设备类型
  const ua = navigator.userAgent
  if (/Mobi|Android|iPhone|iPad|iPod/i.test(ua)) {
    hardwareInfo.value.deviceType = '移动设备'
  } else if (/Tablet|iPad/i.test(ua)) {
    hardwareInfo.value.deviceType = '平板设备'
  } else {
    hardwareInfo.value.deviceType = '桌面设备'
  }
}

function collectNetworkInfo() {
  // @ts-ignore
  const connection = navigator.connection || navigator.mozConnection || navigator.webkitConnection
  if (connection) {
    networkInfo.value.effectiveType = connection.effectiveType || '未知'
    networkInfo.value.downlink = connection.downlink ? `${connection.downlink} Mbps` : '未知'
    networkInfo.value.rtt = connection.rtt ? `${connection.rtt} ms` : '未知'
    networkInfo.value.saveData = connection.saveData || false
  } else {
    networkInfo.value.effectiveType = '不支持'
    networkInfo.value.downlink = '不支持'
    networkInfo.value.rtt = '不支持'
  }
}

function collectDisplayInfo() {
  displayInfo.value.windowSize = `${window.innerWidth} × ${window.innerHeight}`
  displayInfo.value.documentSize = `${document.documentElement.scrollWidth} × ${document.documentElement.scrollHeight}`
  displayInfo.value.colorDepth = `${screen.colorDepth} 位`

  // @ts-ignore
  const orientation = screen.orientation || screen.mozOrientation || screen.msOrientation
  displayInfo.value.orientation = orientation ? orientation.type : '未知'
}

function checkFeatureSupport() {
  featureSupport.value = {
    'Service Worker': 'serviceWorker' in navigator,
    'Push API': 'PushManager' in window,
    'Notification': 'Notification' in window,
    'Geolocation': 'geolocation' in navigator,
    'Camera': 'mediaDevices' in navigator,
    'WebGL': !!document.createElement('canvas').getContext('webgl'),
    'WebGL2': !!document.createElement('canvas').getContext('webgl2'),
    'WebAssembly': typeof WebAssembly === 'object',
    'SharedArrayBuffer': typeof SharedArrayBuffer === 'function',
    'WebSocket': 'WebSocket' in window,
    'Fetch API': 'fetch' in window,
    'Web Workers': 'Worker' in window,
    'IndexedDB': 'indexedDB' in window,
    'LocalStorage': 'localStorage' in window,
    'SessionStorage': 'sessionStorage' in window,
    'WebRTC': 'RTCPeerConnection' in window,
    'Bluetooth': 'bluetooth' in navigator,
    'USB': 'usb' in navigator,
    'Gamepad': 'getGamepads' in navigator,
    'Clipboard API': 'clipboard' in navigator,
  }
}

function copyAllInfo() {
  const info = {
    浏览器: browserInfo.value,
    操作系统: osInfo.value,
    硬件: hardwareInfo.value,
    网络: networkInfo.value,
    显示: displayInfo.value,
    功能支持: featureSupport.value,
  }

  navigator.clipboard.writeText(JSON.stringify(info, null, 2)).then(() => {
    message.success('已复制到剪贴板')
  })
}
</script>

<style scoped lang="scss">
.system-info-tool {
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
