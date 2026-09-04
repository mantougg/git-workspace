<template>
  <div class="about-view">
    <Panel title="关于" class="about-panel">
      <section class="about-identity" aria-labelledby="about-app-name">
        <img class="about-icon" :src="appIcon" alt="GitWorkspace 应用图标" />
        <div class="about-identity-copy">
          <h1 id="about-app-name">GitWorkspace</h1>
          <p>v{{ appVersion }} by {{ appAuthor }}</p>
        </div>
      </section>

      <n-divider />

      <section class="about-section" aria-labelledby="app-info-title">
        <h2 id="app-info-title">应用信息</h2>
        <div class="about-info-list">
          <div class="about-info-row">
            <span class="about-info-label">GitHub 仓库</span>
            <span class="about-info-value about-mono">{{ repositoryUrl }}</span>
            <n-button
              text
              type="primary"
              title="在浏览器中打开 GitHub 仓库"
              aria-label="在浏览器中打开 GitHub 仓库"
              @click="openExternalUrl(repositoryUrl)"
            >
              <template #icon><n-icon><OpenOutline /></n-icon></template>
            </n-button>
          </div>
          <div class="about-info-row">
            <span class="about-info-label">开源协议</span>
            <span class="about-info-value">{{ appLicense }} License</span>
            <n-button text type="primary" @click="openExternalUrl(licenseUrl)">
              查看协议
              <template #icon><n-icon><OpenOutline /></n-icon></template>
            </n-button>
          </div>
        </div>
      </section>

      <n-divider />

      <section class="about-section" aria-labelledby="update-title">
        <div class="about-update-heading">
          <div>
            <h2 id="update-title">软件更新</h2>
            <p class="about-current-version">当前版本 v{{ appVersion }}</p>
          </div>
          <n-button
            type="primary"
            :loading="status === 'checking'"
            :disabled="status === 'downloading' || status === 'ready'"
            @click="checkForUpdates"
          >
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            检查更新
          </n-button>
        </div>

        <n-alert v-if="status === 'upToDate'" type="info" :show-icon="true">
          当前已是最新版本
        </n-alert>

        <div v-else-if="status === 'available'" class="about-update-available">
          <div class="about-update-version">发现新版本 v{{ updateVersion }}</div>
          <pre v-if="updateBody" class="about-update-body">{{ updateBody }}</pre>
          <n-button type="primary" @click="downloadAndInstall">
            <template #icon><n-icon><DownloadOutline /></n-icon></template>
            下载并安装
          </n-button>
        </div>

        <div v-else-if="status === 'downloading'" class="about-download">
          <span>正在下载并安装 v{{ updateVersion }}</span>
          <n-progress
            type="line"
            :percentage="downloadProgress ?? 0"
            :show-indicator="downloadProgress !== null"
            :processing="downloadProgress === null"
          />
        </div>

        <n-alert v-else-if="status === 'ready'" type="success" :show-icon="true">
          <div class="about-ready">
            <span>更新已安装，重启应用后生效</span>
            <n-button type="primary" @click="restartApp">
              <template #icon><n-icon><PowerOutline /></n-icon></template>
              立即重启
            </n-button>
          </div>
        </n-alert>

        <n-alert v-else-if="status === 'error'" type="error" :show-icon="true">
          <div class="about-error">
            <span>{{ error }}</span>
            <n-button text type="primary" @click="openExternalUrl(releasesUrl)">
              前往 GitHub Releases 手动下载
              <template #icon><n-icon><OpenOutline /></n-icon></template>
            </n-button>
          </div>
        </n-alert>
      </section>

      <!-- T-35：诊断与反馈闭环（崩溃报告 / 反馈包 / 遥测 opt-in） -->
      <n-divider />

      <section class="about-section" aria-labelledby="diagnostics-title">
        <h2 id="diagnostics-title">诊断与反馈</h2>
        <div class="about-info-list">
          <div class="about-info-row">
            <span class="about-info-label">崩溃报告</span>
            <span class="about-info-value">{{ crashReports.length }} 份</span>
            <n-button text type="primary" @click="loadCrashReports">刷新</n-button>
            <n-button v-if="crashReports.length > 0" text type="error" @click="clearReports">
              清空
            </n-button>
          </div>
          <div class="about-info-row">
            <span class="about-info-label">反馈包（日志 + 崩溃报告）</span>
            <n-button text type="primary" :loading="bundleBusy" @click="makeBundle">
              一键导出
            </n-button>
          </div>
          <div class="about-info-row">
            <span class="about-info-label">
              匿名遥测<span class="about-telemetry-hint">（默认关闭；数据脱敏后仅本地留存）</span>
            </span>
            <n-switch v-model:value="telemetry.enabled" size="small" @update:value="saveTelemetry" />
          </div>
        </div>
      </section>
      <!-- F-38：数据管理（清除历史与缓存，配置保留，二次确认） -->
      <n-divider />

      <section class="about-section" aria-labelledby="data-title">
        <h2 id="data-title">数据</h2>
        <div class="about-info-list">
          <div class="about-info-row">
            <span class="about-info-label">
              本地数据<span class="about-telemetry-hint">（清除历史与缓存；工作区、JDK、Runtime 配置等保留）</span>
            </span>
            <span class="about-info-value" />
            <n-button
              text
              type="error"
              :loading="clearingData"
              @click="confirmClearData"
            >
              清除数据
            </n-button>
          </div>
        </div>
      </section>
    </Panel>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { open as tauriOpen } from "@tauri-apps/plugin-shell";
import { NButton, NIcon, useMessage } from "naive-ui";
import {
  DownloadOutline,
  OpenOutline,
  PowerOutline,
  RefreshOutline,
} from "@vicons/ionicons5";
import Panel from "@/components/shell/Panel.vue";
// Source: src-tauri/icons/128x128.png, copied for frontend bundling.
import appIcon from "@/assets/app-icon.svg";
import { useUpdater } from "@/composables/useUpdater";
import { clearCachedData } from "@/api/app";

const appVersion = __APP_VERSION__;
const appAuthor = __APP_AUTHOR__;
const appLicense = __APP_LICENSE__;
const appRepository = __APP_REPOSITORY__;
const message = useMessage();
const {
  status,
  updateVersion,
  updateBody,
  downloadProgress,
  error,
  checkForUpdates,
  downloadAndInstall,
  restartApp,
} = useUpdater();

function normalizeRepositoryUrl(repository: string): string {
  const value = repository.trim().replace(/\.git$/, "");
  if (value.startsWith("github:")) {
    return `https://github.com/${value.slice("github:".length)}`;
  }
  if (value.startsWith("git+")) return value.slice(4);
  return value;
}

const repositoryUrl = computed(() => normalizeRepositoryUrl(appRepository));
const releasesUrl = computed(() => `${repositoryUrl.value}/releases/latest`);
const licenseUrl = computed(() => `${repositoryUrl.value}/blob/HEAD/LICENSE`);

async function openExternalUrl(url: string) {
  try {
    await tauriOpen(url);
  } catch (cause) {
    message.error(cause instanceof Error ? cause.message : String(cause));
  }
}

// ── T-35：诊断与反馈 ─────────────────────────────────────────
import { onMounted, ref } from "vue";
import { NAlert, NDivider, NProgress, NSwitch, useDialog } from "naive-ui";
import {
  clearCrashReports,
  collectFeedbackBundle,
  getCrashReports,
  getTelemetryConfig,
  setTelemetryConfig,
  type CrashReportInfo,
  type TelemetrySettings,
} from "@/api/diagnostics";

const crashReports = ref<CrashReportInfo[]>([]);
const bundleBusy = ref(false);
const telemetry = ref<TelemetrySettings>({ enabled: false, crashUpload: false });

onMounted(async () => {
  await loadCrashReports();
  try {
    telemetry.value = await getTelemetryConfig();
  } catch {
    // 默认关闭
  }
});

async function loadCrashReports() {
  try {
    crashReports.value = await getCrashReports();
  } catch (e) {
    message.error("读取崩溃报告失败: " + (e instanceof Error ? e.message : String(e)));
  }
}

async function clearReports() {
  try {
    await clearCrashReports();
    crashReports.value = [];
    message.success("已清空崩溃报告");
  } catch (e) {
    message.error("清空失败: " + (e instanceof Error ? e.message : String(e)));
  }
}

async function makeBundle() {
  bundleBusy.value = true;
  try {
    const path = await collectFeedbackBundle();
    message.success("反馈包已导出：" + path);
  } catch (e) {
    message.error("导出失败: " + (e instanceof Error ? e.message : String(e)));
  } finally {
    bundleBusy.value = false;
  }
}

async function saveTelemetry(enabled: boolean) {
  try {
    await setTelemetryConfig({ ...telemetry.value, enabled });
    message.success(enabled ? "遥测已开启（数据脱敏，仅本地留存）" : "遥测已关闭");
  } catch (e) {
    message.error("保存失败: " + (e instanceof Error ? e.message : String(e)));
  }
}

// ── F-38：清除数据（历史与缓存，配置保留，二次确认）────────────
const dialog = useDialog();
const clearingData = ref(false);

function confirmClearData() {
  dialog.error({
    title: "确认清除数据",
    content:
      "将清除运行历史、仓库 / 符号 / Maven 索引、AI 历史与缓存等可重建数据；" +
      "工作区、JDK、Runtime 配置等手动配置保留。清除后可通过重新扫描 / 解析依赖重建索引。确定继续吗？",
    positiveText: "清除",
    negativeText: "取消",
    onPositiveClick: async () => {
      clearingData.value = true;
      try {
        const results = await clearCachedData();
        const total = results.reduce((sum, r) => sum + r.deleted, 0);
        message.success(`已清除 ${total} 行历史与缓存数据`);
      } catch (e) {
        message.error("清除失败: " + (e instanceof Error ? e.message : String(e)));
      } finally {
        clearingData.value = false;
      }
    },
  });
}
</script>

<style scoped>
.about-view {
  height: 100%;
  overflow: auto;
  padding: var(--gw-space-4);
}

.about-panel {
  width: 100%;
}

.about-identity {
  display: flex;
  align-items: center;
  gap: var(--gw-space-4);
  min-height: var(--gw-icon-xl);
}

.about-icon {
  width: var(--gw-icon-xl);
  height: var(--gw-icon-xl);
  flex: 0 0 var(--gw-icon-xl);
}

.about-identity-copy {
  min-width: 0;
}

.about-identity-copy h1 {
  color: var(--gw-text);
  font-size: var(--gw-text-lg);
  font-weight: 600;
}

.about-identity-copy p,
.about-current-version {
  margin-top: var(--gw-space-1);
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.about-section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.about-section h2 {
  color: var(--gw-text);
  font-size: var(--gw-text-md);
  font-weight: 600;
}

.about-info-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.about-info-row {
  display: grid;
  grid-template-columns: var(--gw-about-label-w) minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--gw-space-3);
  min-height: var(--gw-space-4);
}

.about-info-label {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.about-info-value {
  min-width: 0;
  overflow: hidden;
  color: var(--gw-text);
  font-size: var(--gw-text-sm);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.about-mono,
.about-update-body {
  font-family: var(--gw-font-mono);
}

.about-update-heading,
.about-ready,
.about-error {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-3);
}

.about-update-heading > div,
.about-ready span,
.about-error span {
  min-width: 0;
}

.about-update-available,
.about-download {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.about-update-version {
  color: var(--gw-text);
  font-size: var(--gw-text-md);
  font-weight: 600;
}

.about-update-body {
  max-height: var(--gw-about-body-max-h);
  overflow: auto;
  padding: var(--gw-space-3);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-sm);
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

@media (max-width: 640px) {
  .about-info-row {
    grid-template-columns: var(--gw-about-label-w) minmax(0, 1fr);
  }

  .about-info-row .n-button {
    grid-column: 2;
    justify-self: start;
  }

  .about-update-heading,
  .about-ready,
  .about-error {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
