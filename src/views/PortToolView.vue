<template>
  <div class="port-tool-view">
    <!-- 查询区 -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-input-number
          v-model:value="port"
          :min="1"
          :max="65535"
          :show-button="false"
          placeholder="输入端口，如 8080"
          style="width: 200px"
          @keyup.enter="onCheck"
        />
        <n-button type="primary" :loading="checking" :disabled="!port" @click="onCheck">
          <template #icon><n-icon><SearchOutline /></n-icon></template>
          查询占用进程
        </n-button>
      </div>
      <div class="toolbar-right hint">查询该端口当前活动进程（PID / 进程名 / 可执行路径），并可选择终止</div>
    </div>

    <div v-if="result" class="result-area">
      <!-- 空闲态 -->
      <n-alert v-if="!result.inUse" type="success" :show-icon="true">
        端口 {{ result.port }} 当前空闲，无进程占用。
      </n-alert>

      <!-- 占用态 -->
      <template v-else>
        <n-alert type="warning" :show-icon="true">
          端口 {{ result.port }} 已被进程占用
        </n-alert>

        <n-card
          v-if="result.occupier"
          size="small"
          class="process-card"
          title="占用进程"
        >
          <n-descriptions :column="1" label-placement="left" size="small">
            <n-descriptions-item label="PID">
              <span class="mono">{{ result.occupier.pid ?? "未知" }}</span>
            </n-descriptions-item>
            <n-descriptions-item label="进程名">
              {{ result.occupier.processName ?? "未知" }}
            </n-descriptions-item>
            <n-descriptions-item label="可执行路径">
              <span class="mono path-cell" :title="result.occupier.executablePath ?? ''">
                {{ result.occupier.executablePath ?? "未知（权限不足或无法解析）" }}
              </span>
            </n-descriptions-item>
          </n-descriptions>

          <template #footer>
            <div class="kill-row">
              <n-popconfirm
                :disabled="!result.occupier?.pid"
                @positive-click="onKill"
              >
                <template #trigger>
                  <n-button
                    type="error"
                    size="small"
                    :loading="killing"
                    :disabled="!result.occupier?.pid"
                  >
                    <template #icon><n-icon><SkullOutline /></n-icon></template>
                    终止进程 (PID {{ result.occupier.pid }})
                  </n-button>
                </template>
                确认终止
                <span class="mono">{{ result.occupier?.processName ?? "未知进程" }}</span>
                (PID {{ result.occupier?.pid }})？该操作不可撤销，可能影响正在运行的
                应用或服务。
              </n-popconfirm>
              <n-button
                v-if="result.occupier?.executablePath"
                size="small"
                @click="openContainingFolder"
              >
                打开所在目录
              </n-button>
            </div>
          </template>
        </n-card>
        <n-alert
          v-else
          type="error"
          :show-icon="true"
        >
          端口被占用，但本机缺少 netstat / lsof，无法定位占用进程；请用系统工具（任务管理器 / lsof）确认。
        </n-alert>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { NAlert, NButton, NCard, NDescriptions, NDescriptionsItem, NIcon, NInputNumber, NPopconfirm, useMessage } from "naive-ui";
import { SearchOutline, SkullOutline } from "@vicons/ionicons5";
import { runtimeCheckPort, runtimeKillPortProcess } from "@/api/runtime";
import type { PortCheckResult } from "@/types/runtime";
import { errMsg } from "@/utils/error";

const message = useMessage();

const port = ref<number | null>(null);
const result = ref<PortCheckResult | null>(null);
const checking = ref(false);
const killing = ref(false);

async function onCheck() {
  if (!port.value) return;
  checking.value = true;
  result.value = null;
  try {
    result.value = await runtimeCheckPort(port.value);
  } catch (e) {
    message.error("查询失败：" + errMsg(e));
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
    // 杀完后重新检测端口状态，确认释放。
    await onCheck();
  } catch (e) {
    message.error("终止进程失败：" + errMsg(e));
  } finally {
    killing.value = false;
  }
}

/** 打开占用进程所在目录（便于人工确认进程归属）。 */
async function openContainingFolder() {
  const exe = result.value?.occupier?.executablePath;
  if (!exe) return;
  try {
    const { open } = await import("@tauri-apps/plugin-shell");
    // 可执行文件存在即父目录存在；去尾段取目录。
    const dir = exe.replace(/[\\/][^\\/]*$/, "") || "/";
    await open(dir);
  } catch (e) {
    message.error("打开目录失败：" + errMsg(e));
  }
}
</script>

<style scoped>
.port-tool-view {
  padding: 16px 24px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2);
}
.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}
.result-area {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  max-width: 720px;
}
.process-card {
  margin-top: var(--gw-space-1);
}
.mono {
  font-family: var(--gw-font-mono);
  font-size: var(--gw-text-sm);
}
.path-cell {
  word-break: break-all;
  white-space: normal;
}
.kill-row {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
</style>
