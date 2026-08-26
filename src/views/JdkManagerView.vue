<template>
  <div class="jdk-manager-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button text @click="goBack">
          <template #icon><n-icon><ArrowBackOutline /></n-icon></template>
          返回
        </n-button>
        <n-button type="primary" :loading="discovering" @click="onDiscover">
          <template #icon><n-icon><RefreshOutline /></n-icon></template>
          扫描本机 JDK
        </n-button>
        <n-button :loading="pruning" @click="onPrune">
          <template #icon><n-icon><TrashOutline /></n-icon></template>
          清理失效条目
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-button type="success" dashed @click="onAddManual">
          <template #icon><n-icon><AddOutline /></n-icon></template>
          手动添加 JDK
        </n-button>
      </div>
    </div>

    <!-- Summary -->
    <div class="summary">
      <span class="summary-item">
        共 <b>{{ jdks.length }}</b> 个
      </span>
      <span class="summary-item valid">
        有效 <b>{{ validCount }}</b>
      </span>
      <span class="summary-item invalid">
        失效 <b>{{ invalidCount }}</b>
      </span>
      <span v-if="jdks.length > 0" class="summary-hint">
        列表按「有效优先 → major 降序 → 路径升序」排列
      </span>
    </div>

    <!-- JDK table -->
    <n-spin :show="loading">
      <n-data-table
        :columns="columns"
        :data="jdks"
        :row-key="(row: JdkInstallation) => row.id"
        empty-text="未发现 JDK，请点击「扫描本机 JDK」或手动添加"
      />
    </n-spin>

    <!-- Raw version output (collapsible, for vendor discrepancy debugging) -->
    <n-collapse v-if="jdks.some((j) => j.rawVersion)" class="raw-collapse">
      <n-collapse-item title="原始 java -version 输出（排查厂商差异）" name="raw">
        <div v-for="j in jdks.filter((x) => x.rawVersion)" :key="j.id" class="raw-row">
          <div class="raw-head">
            <span class="mono">{{ j.homePath }}</span>
            <n-tag :type="j.isValid ? 'success' : 'error'" size="small">
              {{ j.isValid ? "有效" : "失效" }}
            </n-tag>
          </div>
          <pre class="raw-pre">{{ j.rawVersion }}</pre>
        </div>
      </n-collapse-item>
    </n-collapse>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { NButton, NIcon, NTag, useMessage } from "naive-ui";
import { ArrowBackOutline, RefreshOutline, TrashOutline, AddOutline } from "@vicons/ionicons5";
import {
  addJdkManualByPicker,
  discoverJdks,
  listJdks,
  pruneInvalidJdks,
  removeJdk,
  validateJdk,
} from "@/api/jdk";
import type { JdkDiscoverySource, JdkInstallation, JdkVendor } from "@/types/jdk";
import { errMsg } from "@/utils/error";

const router = useRouter();
const message = useMessage();

const jdks = ref<JdkInstallation[]>([]);
const loading = ref(false);
const discovering = ref(false);
const pruning = ref(false);
const validatingId = ref<number | null>(null);

const validCount = computed(() => jdks.value.filter((j) => j.isValid).length);
const invalidCount = computed(() => jdks.value.filter((j) => !j.isValid).length);

const columns = [
  {
    title: "状态",
    width: 90,
    render(row: JdkInstallation) {
      return h(NTag, { type: row.isValid ? "success" : "error", size: "small", bordered: false }, { default: () => row.isValid ? "有效" : "失效" });
    },
  },
  {
    title: "Major",
    width: 80,
    align: "center" as const,
    render(row: JdkInstallation) {
      if (row.majorVersion != null) {
        return h("span", { class: "major-badge" }, row.majorVersion);
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "厂商",
    width: 110,
    render(row: JdkInstallation) {
      if (row.vendor) {
        return h("span", null, vendorLabel(row.vendor));
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "完整版本",
    minWidth: 130,
    render(row: JdkInstallation) {
      if (row.fullVersion) {
        return h("span", null, row.fullVersion);
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "来源",
    width: 100,
    render(row: JdkInstallation) {
      return h(NTag, { size: "small", type: "info", bordered: true }, { default: () => sourceLabel(row.source) });
    },
  },
  {
    title: "架构 / 位宽",
    width: 120,
    render(row: JdkInstallation) {
      if (row.architecture || row.bitness) {
        return h("span", null, `${row.architecture || "?"} / ${row.bitness || "?"}bit`);
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "JDK 根目录",
    minWidth: 260,
    ellipsis: { tooltip: true },
    render(row: JdkInstallation) {
      return h("span", { class: "mono" }, row.homePath);
    },
  },
  {
    title: "最近校验",
    width: 170,
    render(row: JdkInstallation) {
      if (row.lastChecked) {
        return h("span", { class: "muted" }, formatTime(row.lastChecked));
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "操作",
    width: 180,
    fixed: "right" as const,
    render(row: JdkInstallation) {
      return h("div", { style: "display: flex; gap: 8px;" }, [
        h(
          NButton,
          {
            size: "small",
            loading: validatingId.value === row.id,
            onClick: () => onValidate(row),
          },
          { default: () => "复检" }
        ),
        h(
          NButton,
          {
            size: "small",
            type: "error",
            dashed: true,
            onClick: () => onRemove(row),
          },
          { default: () => "删除" }
        ),
      ]);
    },
  },
];

async function reload() {
  loading.value = true;
  try {
    jdks.value = await listJdks();
  } catch (e) {
    message.error("加载 JDK 列表失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function onDiscover() {
  discovering.value = true;
  try {
    const count = await discoverJdks();
    message.success(`发现并入库 ${count} 个 JDK`);
    await reload();
  } catch (e) {
    message.error("扫描 JDK 失败：" + errMsg(e));
  } finally {
    discovering.value = false;
  }
}

async function onPrune() {
  pruning.value = true;
  try {
    const n = await pruneInvalidJdks();
    if (n > 0) {
      message.success(`已标记 ${n} 个失效条目（路径已不存在）`);
    } else {
      message.info("无失效条目需要清理");
    }
    await reload();
  } catch (e) {
    message.error("清理失效条目失败：" + errMsg(e));
  } finally {
    pruning.value = false;
  }
}

async function onAddManual() {
  try {
    const added = await addJdkManualByPicker();
    if (!added) {
      return; // 用户取消选择
    }
    message.success(
      added.isValid
        ? `已添加 JDK ${added.majorVersion ?? "?"}（${added.fullVersion ?? ""}）`
        : `已添加但版本探测失败（is_valid=false），请查看原始输出排查`
    );
    await reload();
  } catch (e) {
    // JdkNotFound 等可行动错误在此展示后端给出的提示。
    message.error("添加 JDK 失败：" + errMsg(e));
  }
}

async function onValidate(row: JdkInstallation) {
  if (row.id == null) return;
  validatingId.value = row.id;
  try {
    const updated = await validateJdk(row.id);
    const idx = jdks.value.findIndex((j) => j.id === row.id);
    if (idx >= 0) jdks.value[idx] = updated;
    message.success(
      updated.isValid
        ? `复检通过：major=${updated.majorVersion ?? "?"}`
        : `复检失败，已标记失效`
    );
  } catch (e) {
    message.error("复检失败：" + errMsg(e));
  } finally {
    validatingId.value = null;
  }
}

async function onRemove(row: JdkInstallation) {
  if (row.id == null) return;
  try {
    await removeJdk(row.id);
    jdks.value = jdks.value.filter((j) => j.id !== row.id);
    message.success("已删除");
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
  }
}

function goBack() {
  router.push({ name: "dashboard" });
}

const SOURCE_LABELS: Record<JdkDiscoverySource, string> = {
  system: "系统扫描",
  javaHome: "JAVA_HOME",
  path: "PATH",
  mise: "mise",
  jenv: "jEnv",
  sdkman: "SDKMAN",
  manual: "手动",
};

function sourceLabel(s: JdkDiscoverySource): string {
  return SOURCE_LABELS[s] ?? s;
}

const VENDOR_LABELS: Record<JdkVendor, string> = {
  oracle: "Oracle",
  openJdk: "OpenJDK",
  temurin: "Temurin",
  corretto: "Corretto",
  graalVm: "GraalVM",
  zulu: "Zulu",
  liberica: "Liberica",
  other: "其他",
};

function vendorLabel(v: JdkVendor): string {
  return VENDOR_LABELS[v] ?? v;
}

function formatTime(iso: string): string {
  if (!iso) return "—";
  // RFC3339 -> 本地可读；解析失败回退原值。
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

onMounted(reload);
</script>

<style scoped>
.jdk-manager-view {
  padding: 16px 24px;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 16px;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  align-items: center;
}
.summary {
  display: flex;
  gap: 20px;
  align-items: center;
  margin-bottom: 12px;
  font-size: 14px;
  color: var(--el-text-color-regular);
}
.summary-item b {
  color: var(--el-color-primary);
  margin: 0 2px;
}
.summary-item.valid b {
  color: var(--el-color-success);
}
.summary-item.invalid b {
  color: var(--el-color-danger);
}
.summary-hint {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}
.major-badge {
  font-weight: 600;
  color: var(--el-color-primary);
}
.muted {
  color: var(--el-text-color-secondary);
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  font-size: 12px;
}
.raw-collapse {
  margin-top: 16px;
}
.raw-row {
  margin-bottom: 12px;
}
.raw-head {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-bottom: 4px;
}
.raw-pre {
  background: var(--el-fill-color-light);
  padding: 8px 12px;
  border-radius: 4px;
  font-size: 12px;
  white-space: pre-wrap;
  margin: 0;
  max-height: 160px;
  overflow: auto;
}
</style>
