<template>
  <n-modal
    v-model:show="visible"
    preset="card"
    title="端口诊断 · Port Manager"
    class="port-modal"
    :style="{ width: '560px' }"
  >
    <n-space vertical :size="16">
      <n-form-item label="端口" label-placement="left" :show-feedback="false">
        <n-space>
          <n-input-number
            v-model:value="port"
            :min="1"
            :max="65535"
            :show-button="false"
            style="width: 140px"
            placeholder="如 8080"
          />
          <n-button type="primary" :loading="checking" :disabled="!port" @click="onCheck">
            检查占用
          </n-button>
        </n-space>
      </n-form-item>

      <n-alert v-if="result" :type="result.inUse ? 'warning' : 'success'" :show-icon="true">
        <template #header>
          {{ result.inUse ? `端口 ${result.port} 已被占用` : `端口 ${result.port} 空闲` }}
        </template>
        <div v-if="result.occupier" class="occupier">
          <div>
            占用进程：
            <span class="mono">
              {{ result.occupier.processName ?? "未知进程" }} (PID
              {{ result.occupier.pid ?? "未知" }})
            </span>
          </div>
        </div>
        <div v-else-if="result.inUse" class="occupier">
          占用方信息不可用（本机缺少 lsof / netstat），请用系统工具确认进程身份。
        </div>
      </n-alert>

      <template v-if="result?.inUse && result.occupier?.pid">
        <n-alert type="error" :show-icon="true">
          终止他人进程属危险操作：请确认该进程身份后执行（TERM 优雅优先，3s 未退出升级
          KILL）。
        </n-alert>
        <n-space>
          <n-popconfirm @positive-click="onKill">
            <template #trigger>
              <n-button type="error" :loading="killing">
                终止进程 (PID {{ result.occupier.pid }})
              </n-button>
            </template>
            确认终止
            <span class="mono">
              {{ result.occupier.processName ?? "未知进程" }} (PID
              {{ result.occupier.pid }})
            </span>
            ？该操作不可撤销。
          </n-popconfirm>
        </n-space>
      </template>

      <n-divider />
      <div class="section-title">改用其他端口启动「{{ runtimeName }}」</div>
      <n-space>
        <n-input-number
          v-model:value="newPort"
          :min="1"
          :max="65535"
          :show-button="false"
          style="width: 140px"
          placeholder="新端口"
        />
        <n-button :loading="changing" :disabled="!newPort" @click="onChangePort">
          写入 Runtime 配置
        </n-button>
      </n-space>
      <div class="hint">
        仅修改 GitWorkspace 的 Runtime 配置（注入 <span class="mono">--server.port=</span>
        参数），不触碰你的项目文件；随后需重新启动应用生效。
      </div>
    </n-space>
  </n-modal>
</template>

<script setup lang="ts">
// R-16 §81 Port Manager UI：端口占用检测 → 占用方识别 → Kill（二次确认）/
// Change Runtime Port。危险操作（Kill）用 popconfirm 二次确认，文案明确进程
// 身份（全局约束 §3）。
import { computed, ref, watch } from "vue";
import {
  NAlert,
  NButton,
  NDivider,
  NFormItem,
  NInputNumber,
  NModal,
  NPopconfirm,
  NSpace,
  useMessage,
} from "naive-ui";
import {
  runtimeChangeRuntimePort,
  runtimeCheckPort,
  runtimeKillPortProcess,
} from "@/api/runtime";
import type { PortCheckResult } from "@/types/runtime";
import { useWorkspaceStore } from "@/stores/workspace";

const props = defineProps<{
  show: boolean;
  runtimeName: string;
  /** 默认检查端口（通常取应用的探测端口或 8080）。 */
  defaultPort?: number | null;
}>();

const emit = defineEmits<{
  (e: "update:show", value: boolean): void;
  /** 端口已写入配置，父组件可提示重启。 */
  (e: "port-changed", port: number): void;
}>();

const message = useMessage();
const workspaceStore = useWorkspaceStore();

const visible = computed({
  get: () => props.show,
  set: (v) => emit("update:show", v),
});

const port = ref<number | null>(props.defaultPort ?? 8080);
const newPort = ref<number | null>(null);
const result = ref<PortCheckResult | null>(null);
const checking = ref(false);
const killing = ref(false);
const changing = ref(false);

watch(
  () => props.show,
  (show) => {
    if (show) {
      port.value = props.defaultPort ?? 8080;
      result.value = null;
    }
  },
);

async function onCheck() {
  if (!port.value) return;
  checking.value = true;
  try {
    result.value = await runtimeCheckPort(port.value);
  } catch (error) {
    message.error(`端口检查失败：${error}`);
  } finally {
    checking.value = false;
  }
}

async function onKill() {
  const pid = result.value?.occupier?.pid;
  if (!pid) return;
  killing.value = true;
  try {
    const outcome = await runtimeKillPortProcess(pid, true);
    if (outcome.killed) {
      message.success(`已终止进程 ${outcome.processName ?? ""} (PID ${pid})`);
    } else {
      message.warning(`进程 ${pid} 已不存在（可能已自行退出）`);
    }
    await onCheck();
  } catch (error) {
    message.error(`终止进程失败：${error}`);
  } finally {
    killing.value = false;
  }
}

async function onChangePort() {
  if (!newPort.value || !workspaceStore.currentWorkspace) return;
  changing.value = true;
  try {
    await runtimeChangeRuntimePort(
      workspaceStore.currentWorkspace.id,
      props.runtimeName,
      newPort.value,
    );
    message.success(
      `已把「${props.runtimeName}」的端口改为 ${newPort.value}，重新启动后生效`,
    );
    emit("port-changed", newPort.value);
    visible.value = false;
  } catch (error) {
    message.error(`改写端口失败：${error}`);
  } finally {
    changing.value = false;
  }
}
</script>

<style scoped>
.port-modal .mono {
  font-family: var(--gw-font-mono, monospace);
}
.occupier {
  margin-top: 4px;
}
.section-title {
  font-weight: 600;
  font-size: var(--gw-text-sm, 13px);
}
.hint {
  color: var(--gw-text-tertiary, #999);
  font-size: var(--gw-text-xs, 12px);
}
</style>
