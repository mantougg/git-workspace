<template>
  <div class="maven-settings-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-button type="primary" :loading="pruning" @click="onPrune">
          <el-icon><Delete /></el-icon>
          清理失效条目
        </el-button>
      </div>
      <div class="toolbar-right">
        <el-button type="success" plain :loading="detecting" @click="onDetectByPicker">
          <el-icon><Search /></el-icon>
          检测项目 Maven
        </el-button>
      </div>
    </div>

    <!-- Local repository info -->
    <el-alert
      class="repo-info"
      :type="localRepo ? 'info' : 'warning'"
      :closable="false"
      show-icon
    >
      <template #title>
        本地仓库路径：<b class="mono">{{ localRepo || "未探测" }}</b>
        <span class="repo-hint">（来自 settings.xml 的 localRepository，无则 ~/.m2/repository）</span>
      </template>
    </el-alert>

    <!-- Summary -->
    <div class="summary">
      <span class="summary-item">
        共 <b>{{ executables.length }}</b> 个
      </span>
      <span class="summary-item valid">
        有效 <b>{{ validCount }}</b>
      </span>
      <span class="summary-item invalid">
        失效 <b>{{ invalidCount }}</b>
      </span>
      <span v-if="executables.length > 0" class="summary-hint">
        优先级：Wrapper > 配置 > 系统
      </span>
    </div>

    <!-- Maven executables table -->
    <el-table
      :data="executables"
      v-loading="loading"
      empty-text="暂无 Maven 可执行体记录，点击「检测项目 Maven」或构建项目后自动入库"
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
      <el-table-column label="完整版本" min-width="120">
        <template #default="{ row }">
          <span v-if="row.fullVersion">{{ row.fullVersion }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="来源" width="120">
        <template #default="{ row }">
          <el-tag size="small" type="info" effect="plain">{{ sourceLabel(row.source) }}</el-tag>
        </template>
      </el-table-column>
      <el-table-column label="所属项目" min-width="200" show-overflow-tooltip>
        <template #default="{ row }">
          <span v-if="row.projectPath" class="mono">{{ row.projectPath }}</span>
          <span v-else class="muted">—</span>
        </template>
      </el-table-column>
      <el-table-column label="可执行路径" min-width="280" show-overflow-tooltip>
        <template #default="{ row }">
          <span class="mono">{{ row.executablePath }}</span>
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
            title="确定删除该 Maven 记录吗？"
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

    <!-- Command preview panel -->
    <el-collapse class="preview-collapse">
      <el-collapse-item title="命令预览（构造 Maven 命令行）" name="preview">
        <div class="preview-form">
          <el-form label-width="100px" size="small">
            <el-form-item label="可执行路径">
              <el-input v-model="previewForm.executable" placeholder="/usr/bin/mvn 或 ./mvnw" />
            </el-form-item>
            <el-form-item label="工作目录">
              <el-input v-model="previewForm.workingDir" placeholder="/path/to/project" />
            </el-form-item>
            <el-form-item label="Goals">
              <el-input v-model="previewForm.goals" placeholder="clean install" />
            </el-form-item>
            <el-form-item label="额外参数">
              <el-input v-model="previewForm.extraArgs" placeholder="-DskipTests -Pprod" />
            </el-form-item>
            <el-form-item label="本地仓库">
              <el-input v-model="previewForm.localRepository" placeholder="留空用默认" />
            </el-form-item>
            <el-form-item>
              <el-button type="primary" :loading="previewing" @click="onPreview">
                生成命令预览
              </el-button>
            </el-form-item>
          </el-form>
          <div v-if="previewResult" class="preview-result">
            <div class="preview-head">完整命令：</div>
            <pre class="preview-pre">{{ previewResult }}</pre>
          </div>
        </div>
      </el-collapse-item>
    </el-collapse>

    <!-- Raw version output (collapsible) -->
    <el-collapse v-if="executables.some((e) => e.rawVersion)" class="raw-collapse">
      <el-collapse-item title="原始 mvn -v 输出（排查版本差异）" name="raw">
        <div v-for="e in executables.filter((x) => x.rawVersion)" :key="e.id" class="raw-row">
          <div class="raw-head">
            <span class="mono">{{ e.executablePath }}</span>
            <el-tag size="small" :type="e.isValid ? 'success' : 'danger'">
              {{ e.isValid ? "有效" : "失效" }}
            </el-tag>
          </div>
          <pre class="raw-pre">{{ e.rawVersion }}</pre>
        </div>
      </el-collapse-item>
    </el-collapse>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { Back, Delete, Search } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import {
  buildMavenCommand,
  detectMavenByPicker,
  listMavenExecutables,
  pruneInvalidMaven,
  removeMavenExecutable,
  resolveLocalRepo,
  validateMavenExecutable,
} from "@/api/maven";
import type { MavenExecutable, MavenSource } from "@/types/maven";
import { errMsg } from "@/utils/error";

const router = useRouter();

const executables = ref<MavenExecutable[]>([]);
const localRepo = ref("");
const loading = ref(false);
const detecting = ref(false);
const pruning = ref(false);
const validatingId = ref<number | null>(null);

const validCount = computed(() => executables.value.filter((e) => e.isValid).length);
const invalidCount = computed(() => executables.value.filter((e) => !e.isValid).length);

const previewForm = reactive({
  executable: "",
  workingDir: "",
  goals: "clean install",
  extraArgs: "-DskipTests",
  localRepository: "",
});
const previewing = ref(false);
const previewResult = ref("");

async function reload() {
  loading.value = true;
  try {
    const [exes, repo] = await Promise.all([
      listMavenExecutables(),
      resolveLocalRepo(),
    ]);
    executables.value = exes;
    localRepo.value = repo;
  } catch (e) {
    ElMessage.error("加载 Maven 列表失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function onDetectByPicker() {
  detecting.value = true;
  try {
    const resolved = await detectMavenByPicker();
    if (!resolved) return; // 用户取消
    ElMessage.success(
      `检测成功：${resolved.executable.fullVersion ?? "?"}（${
        resolved.usesWrapper ? "Wrapper" : sourceLabel(resolved.executable.source)
      }）`,
    );
    await reload();
  } catch (e) {
    // MavenNotFound 等可行动错误在此展示后端给出的提示。
    ElMessage.error("检测 Maven 失败：" + errMsg(e));
  } finally {
    detecting.value = false;
  }
}

async function onPrune() {
  pruning.value = true;
  try {
    const n = await pruneInvalidMaven();
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

async function onValidate(row: MavenExecutable) {
  if (row.id == null) return;
  validatingId.value = row.id;
  try {
    const updated = await validateMavenExecutable(row.id);
    const idx = executables.value.findIndex((e) => e.id === row.id);
    if (idx >= 0) executables.value[idx] = updated;
    ElMessage.success(
      updated.isValid
        ? `复检通过：major=${updated.majorVersion ?? "?"}`
        : `复检失败，已标记失效`,
    );
  } catch (e) {
    ElMessage.error("复检失败：" + errMsg(e));
  } finally {
    validatingId.value = null;
  }
}

async function onRemove(row: MavenExecutable) {
  if (row.id == null) return;
  try {
    await removeMavenExecutable(row.id);
    executables.value = executables.value.filter((e) => e.id !== row.id);
    ElMessage.success("已删除");
  } catch (e) {
    ElMessage.error("删除失败：" + errMsg(e));
  }
}

async function onPreview() {
  previewing.value = true;
  try {
    const req = {
      workingDir: previewForm.workingDir || ".",
      executable: previewForm.executable,
      goals: previewForm.goals.split(/\s+/).filter(Boolean),
      extraArgs: previewForm.extraArgs
        ? previewForm.extraArgs.split(/\s+/).filter(Boolean)
        : [],
      viaCmdC: false,
      localRepository: previewForm.localRepository || null,
    };
    previewResult.value = (await buildMavenCommand(req)).join(" ");
  } catch (e) {
    ElMessage.error("生成预览失败：" + errMsg(e));
  } finally {
    previewing.value = false;
  }
}

function goBack() {
  router.push({ name: "dashboard" });
}

const SOURCE_LABELS: Record<MavenSource, string> = {
  projectWrapper: "Wrapper",
  configured: "配置",
  system: "系统",
};

function sourceLabel(s: MavenSource): string {
  return SOURCE_LABELS[s] ?? s;
}

function formatTime(iso: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

onMounted(reload);
</script>

<style scoped>
.maven-settings-view {
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
.repo-info {
  margin-bottom: 12px;
}
.repo-hint {
  color: var(--el-text-color-secondary);
  font-size: 12px;
  margin-left: 8px;
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
.preview-collapse {
  margin-top: 16px;
}
.preview-form {
  max-width: 720px;
}
.preview-result {
  margin-top: 12px;
}
.preview-head {
  font-size: 13px;
  color: var(--el-text-color-secondary);
  margin-bottom: 4px;
}
.preview-pre {
  background: var(--el-fill-color-light);
  padding: 8px 12px;
  border-radius: 4px;
  font-size: 12px;
  white-space: pre-wrap;
  margin: 0;
  word-break: break-all;
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
