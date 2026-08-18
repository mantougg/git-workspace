<template>
  <div class="jdk-manager-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-button type="primary" :loading="discovering" @click="onDiscover">
          <el-icon><Refresh /></el-icon>
          扫描本机 JDK
        </el-button>
        <el-button :loading="pruning" @click="onPrune">
          <el-icon><Delete /></el-icon>
          清理失效条目
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button type="success" plain @click="onAddManual">
          <el-icon><Plus /></el-icon>
          手动添加 JDK
        </el-button>
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
    <el-table
      :data="jdks"
      v-loading="loading"
      empty-text="未发现 JDK，请点击「扫描本机 JDK」或手动添加"
      row-key="id"
    >
      <el-table-column label="状态" width="90">
        <template #default="{ row }">
          <el-tag :type="row.isValid ? 'success' : 'danger'" size="small" effect="light">
            {{ row.isValid ? "有效" : "失效" }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="Major" width="80" align="center">
        <template #default="{ row }">
          <span v-if="row.majorVersion != null" class="major-badge">{{ row.majorVersion }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="厂商" width="110">
        <template #default="{ row }">
          <span v-if="row.vendor">{{ vendorLabel(row.vendor) }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="完整版本" min-width="130">
        <template #default="{ row }">
          <span v-if="row.fullVersion">{{ row.fullVersion }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="来源" width="100">
        <template #default="{ row }">
          <el-tag size="small" type="info" effect="plain">{{ sourceLabel(row.source) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="架构 / 位宽" width="120">
        <template #default="{ row }">
          <span v-if="row.architecture || row.bitness">
            {{ row.architecture || "?" }} / {{ row.bitness || "?" }}bit
          </span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="JDK 根目录" min-width="260" show-overflow-tooltip>
        <template #default="{ row }">
          <span class="mono">{{ row.homePath }}</span>
        </template>
      </el-table-column>
      <el-table-column label="最近校验" width="170">
        <template #default="{ row }">
          <span v-if="row.lastChecked" class="muted">{{ formatTime(row.lastChecked) }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="180" fixed="right">
        <template #default="{ row }">
          <el-button
            size="small"
            :loading="validatingId === row.id"
            @click="onValidate(row)"
          >
            复检
          </el-button>
          <el-popconfirm
            title="确定删除该 JDK 条目吗？"
            confirm-button-text="删除"
            cancel-button-text="取消"
            @confirm="onRemove(row)"
          >
            <template #reference>
              <el-button size="small" type="danger" plain>删除</el-button>
            </template>
          </el-popconfirm>
        </template>
      </el-table-column>
    </el-table>

    <!-- Raw version output (collapsible, for vendor discrepancy debugging) -->
    <el-collapse v-if="jdks.some((j) => j.rawVersion)" class="raw-collapse">
      <el-collapse-item title="原始 java -version 输出（排查厂商差异）" name="raw">
        <div v-for="j in jdks.filter((x) => x.rawVersion)" :key="j.id" class="raw-row">
          <div class="raw-head">
            <span class="mono">{{ j.homePath }}</span>
            <el-tag size="small" :type="j.isValid ? 'success' : 'danger'">
              {{ j.isValid ? "有效" : "失效" }}
            </el-tag>
          </div>
          <pre class="raw-pre">{{ j.rawVersion }}</pre>
        </div>
      </el-collapse-item>
    </el-collapse>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { Back, Refresh, Delete, Plus } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
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

const jdks = ref<JdkInstallation[]>([]);
const loading = ref(false);
const discovering = ref(false);
const pruning = ref(false);
const validatingId = ref<number | null>(null);

const validCount = computed(() => jdks.value.filter((j) => j.isValid).length);
const invalidCount = computed(() => jdks.value.filter((j) => !j.isValid).length);

async function reload() {
  loading.value = true;
  try {
    jdks.value = await listJdks();
  } catch (e) {
    ElMessage.error("加载 JDK 列表失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function onDiscover() {
  discovering.value = true;
  try {
    const count = await discoverJdks();
    ElMessage.success(`发现并入库 ${count} 个 JDK`);
    await reload();
  } catch (e) {
    ElMessage.error("扫描 JDK 失败：" + errMsg(e));
  } finally {
    discovering.value = false;
  }
}

async function onPrune() {
  pruning.value = true;
  try {
    const n = await pruneInvalidJdks();
    if (n > 0) {
      ElMessage.success(`已标记 ${n} 个失效条目（路径已不存在）`);
    } else {
      ElMessage.info("无失效条目需要清理");
    }
    await reload();
  } catch (e) {
    ElMessage.error("清理失效条目失败：" + errMsg(e));
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
    ElMessage.success(
      added.isValid
        ? `已添加 JDK ${added.majorVersion ?? "?"}（${added.fullVersion ?? ""}）`
        : `已添加但版本探测失败（is_valid=false），请查看原始输出排查`
    );
    await reload();
  } catch (e) {
    // JdkNotFound 等可行动错误在此展示后端给出的提示。
    ElMessage.error("添加 JDK 失败：" + errMsg(e));
  }
}

async function onValidate(row: JdkInstallation) {
  if (row.id == null) return;
  validatingId.value = row.id;
  try {
    const updated = await validateJdk(row.id);
    const idx = jdks.value.findIndex((j) => j.id === row.id);
    if (idx >= 0) jdks.value[idx] = updated;
    ElMessage.success(
      updated.isValid
        ? `复检通过：major=${updated.majorVersion ?? "?"}`
        : `复检失败，已标记失效`
    );
  } catch (e) {
    ElMessage.error("复检失败：" + errMsg(e));
  } finally {
    validatingId.value = null;
  }
}

async function onRemove(row: JdkInstallation) {
  if (row.id == null) return;
  try {
    await removeJdk(row.id);
    jdks.value = jdks.value.filter((j) => j.id !== row.id);
    ElMessage.success("已删除");
  } catch (e) {
    ElMessage.error("删除失败：" + errMsg(e));
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
