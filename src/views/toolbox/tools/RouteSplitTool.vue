<template>
  <div class="route-split-tool">
    <n-alert v-if="unsupported" type="info" :show-icon="true">
      {{ unsupported }}
    </n-alert>

    <template v-else>
      <!-- 网卡总览 -->
      <div class="section">
        <div class="section-head">
          <span class="section-title">本机网卡</span>
          <n-button size="tiny" :loading="loading" @click="load">
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            刷新
          </n-button>
        </div>
        <div v-for="iface in interfaces" :key="iface.ifIndex" class="iface-row">
          <n-tag size="small" :type="iface.connected ? 'success' : 'default'" :bordered="false">
            {{ iface.connected ? "已连接" : "断开" }}
          </n-tag>
          <span class="iface-name">{{ iface.name }}</span>
          <span class="mono dim">#{{ iface.ifIndex }}</span>
          <span class="mono">{{ iface.ips.join("、") || "—" }}</span>
          <span class="dim">网关 {{ iface.gateways.join("、") || "—" }}</span>
          <span class="dim">metric {{ iface.metric ?? "自动" }}</span>
        </div>
        <n-empty v-if="interfaces.length === 0" size="small" description="未读到网卡，点刷新重试" />
      </div>

      <!-- 分流配置 -->
      <div class="section">
        <div class="section-title">分流配置</div>
        <div class="form-row">
          <span class="form-label">内网网卡（网线）</span>
          <n-select
            v-model:value="lanIf"
            size="small"
            :options="lanOptions"
            placeholder="选择连接内网的网卡"
            class="form-select"
          />
        </div>
        <div class="form-row">
          <span class="form-label">内网网关</span>
          <n-select
            v-if="lanGateways.length > 1"
            v-model:value="lanGateway"
            size="small"
            :options="lanGateways.map((g) => ({ label: g, value: g }))"
            class="form-select"
          />
          <span v-else class="mono">{{ lanGateway || "选择内网网卡后自动带出" }}</span>
        </div>
        <div class="form-row">
          <span class="form-label">外网网卡（WiFi）</span>
          <n-select
            v-model:value="wanIf"
            size="small"
            :options="wanOptions"
            placeholder="选择访问外网的网卡"
            class="form-select"
          />
        </div>
        <div class="form-row top">
          <span class="form-label">内网网段</span>
          <n-input
            v-model:value="prefixText"
            type="textarea"
            size="small"
            :autosize="{ minRows: 2, maxRows: 5 }"
            placeholder="每行一个 CIDR，如 10.0.0.0/8"
            class="mono-input"
          />
        </div>
        <div class="hint">
          原理：外网卡 metric 调低让默认路由走 WiFi；内网网段加持久静态路由走网线网关。
        </div>
      </div>

      <!-- 操作 -->
      <div class="actions">
        <n-button type="primary" size="small" :disabled="!planValid" @click="preview(false)">
          应用分流
        </n-button>
        <n-button size="small" :disabled="!planValid" @click="preview(true)">
          恢复默认
        </n-button>
      </div>
    </template>

    <!-- 命令确认（执行前展示全文，触发 UAC） -->
    <n-modal v-model:show="confirmShow">
      <n-card
        :title="confirmRestore ? '恢复默认路由' : '应用路由分流'"
        size="small"
        class="confirm-card"
        :bordered="false"
        role="dialog"
        aria-modal="true"
      >
        <div class="confirm-desc">
          将以<strong>管理员身份</strong>执行以下命令（系统弹出 UAC 请允许）：
        </div>
        <pre class="mono commands">{{ pendingCommands.join("\n") }}</pre>
        <template #footer>
          <div class="confirm-footer">
            <n-button size="small" @click="confirmShow = false">取消</n-button>
            <n-button
              size="small"
              type="primary"
              :loading="applying"
              @click="apply"
            >
              确认执行
            </n-button>
          </div>
        </template>
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  NAlert,
  NButton,
  NCard,
  NEmpty,
  NIcon,
  NInput,
  NModal,
  NSelect,
  NTag,
  useMessage,
} from "naive-ui";
import { RefreshOutline } from "@vicons/ionicons5";
import {
  toolboxListNetInterfaces,
  toolboxRouteApply,
  toolboxRoutePlanPreview,
  type NetInterface,
  type SplitPlan,
} from "@/api/toolbox";
import { errMsg } from "@/utils/error";

const message = useMessage();

/** 表单持久化（恢复默认要用同一份配置）。 */
const STORAGE_KEY = "gw-toolbox-route-split";

const unsupported = ref("");
const interfaces = ref<NetInterface[]>([]);
const loading = ref(false);

const lanIf = ref<number | null>(null);
const lanGateway = ref("");
const wanIf = ref<number | null>(null);
const prefixText = ref("10.0.0.0/8\n172.16.0.0/12");

const confirmShow = ref(false);
const confirmRestore = ref(false);
const pendingCommands = ref<string[]>([]);
const applying = ref(false);

async function load() {
  loading.value = true;
  try {
    interfaces.value = await toolboxListNetInterfaces();
  } catch (e) {
    const msg = errMsg(e);
    if (msg.includes("仅支持")) {
      unsupported.value = msg;
    } else {
      message.error("读取网卡失败：" + msg);
    }
  } finally {
    loading.value = false;
  }
}

const connectedIfaces = computed(() => interfaces.value.filter((i) => i.connected));

/** 内网候选：必须有默认网关（静态路由要经它转发）。 */
const lanOptions = computed(() =>
  connectedIfaces.value
    .filter((i) => i.gateways.length > 0 && i.ifIndex !== wanIf.value)
    .map((i) => ({ label: `${i.name}（${i.ips[0] ?? "?"}）`, value: i.ifIndex })),
);

const wanOptions = computed(() =>
  connectedIfaces.value
    .filter((i) => i.ifIndex !== lanIf.value)
    .map((i) => ({ label: `${i.name}（${i.ips[0] ?? "?"}）`, value: i.ifIndex })),
);

const lanGateways = computed(
  () =>
    connectedIfaces.value.find((i) => i.ifIndex === lanIf.value)?.gateways ?? [],
);

watch(lanIf, () => {
  lanGateway.value = lanGateways.value[0] ?? "";
});

const prefixes = computed(() =>
  prefixText.value
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean),
);

const plan = computed<SplitPlan | null>(() => {
  if (lanIf.value == null || wanIf.value == null || !lanGateway.value) return null;
  return {
    lanIf: lanIf.value,
    lanGateway: lanGateway.value,
    wanIf: wanIf.value,
    prefixes: prefixes.value,
  };
});

const planValid = computed(() => plan.value !== null && prefixes.value.length > 0);

async function preview(restore: boolean) {
  const p = plan.value;
  if (!p) return;
  try {
    pendingCommands.value = await toolboxRoutePlanPreview(p, restore);
    confirmRestore.value = restore;
    confirmShow.value = true;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(p));
  } catch (e) {
    message.error("生成命令失败：" + errMsg(e));
  }
}

async function apply() {
  applying.value = true;
  try {
    await toolboxRouteApply(pendingCommands.value);
    confirmShow.value = false;
    message.success(confirmRestore.value ? "已恢复默认路由" : "分流已生效");
    await load();
  } catch (e) {
    message.error(errMsg(e));
  } finally {
    applying.value = false;
  }
}

onMounted(async () => {
  // 恢复上次配置（恢复默认时需要同一份前缀/网卡）。
  const raw = localStorage.getItem(STORAGE_KEY);
  if (raw) {
    try {
      const p = JSON.parse(raw) as SplitPlan;
      lanIf.value = p.lanIf;
      lanGateway.value = p.lanGateway;
      wanIf.value = p.wanIf;
      prefixText.value = p.prefixes.join("\n");
    } catch {
      // 旧格式损坏则忽略，用默认值。
    }
  }
  await load();
});
</script>

<style scoped>
.route-split-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-4);
  max-width: 720px;
}

.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.section-title {
  font-size: var(--gw-text-sm);
  font-weight: 600;
  color: var(--gw-text-dim);
}

.iface-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  flex-wrap: wrap;
  padding: var(--gw-space-2);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
  font-size: var(--gw-text-sm);
}

.iface-name {
  font-weight: 600;
}

.dim {
  color: var(--gw-text-dim);
}

.form-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.form-row.top {
  align-items: flex-start;
}

.form-label {
  width: 120px;
  flex-shrink: 0;
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.form-select {
  flex: 1;
  max-width: 320px;
}

.mono-input :deep(textarea) {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.actions {
  display: flex;
  gap: var(--gw-space-2);
}

.confirm-card {
  max-width: 640px;
}

.confirm-desc {
  font-size: var(--gw-text-sm);
  margin-bottom: var(--gw-space-2);
}

.commands {
  margin: 0;
  padding: var(--gw-space-3);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
  white-space: pre-wrap;
  word-break: break-all;
}

.confirm-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--gw-space-2);
}

.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
</style>
