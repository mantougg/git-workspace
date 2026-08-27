<template>
  <div class="maven-settings-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button type="primary" :loading="pruning" @click="onPrune">
          <template #icon><n-icon><TrashOutline /></n-icon></template>
          清理失效条目
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-button type="success" dashed :loading="detecting" @click="onDetectByPicker">
          <template #icon><n-icon><SearchOutline /></n-icon></template>
          检测项目 Maven
        </n-button>
      </div>
    </div>

    <!-- Local repository info -->
    <n-alert
      class="repo-info"
      :type="localRepo ? 'info' : 'warning'"
      :closable="false"
      :show-icon="true"
    >
      <template #header>
        本地仓库路径：<b class="mono">{{ localRepo || "未探测" }}</b>
        <span class="repo-hint">（来自 settings.xml 的 localRepository，无则 ~/.m2/repository）</span>
      </template>
    </n-alert>

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
    <n-spin :show="loading">
      <n-data-table
        :data="executables"
        :columns="tableColumns"
        :row-key="(row: MavenExecutable) => row.id ?? ''"
        empty-text="暂无 Maven 可执行体记录，点击「检测项目 Maven」或构建项目后自动入库"
      />
    </n-spin>

    <!-- Command preview panel -->
    <n-collapse class="preview-collapse">
      <n-collapse-item title="命令预览（构造 Maven 命令行）" name="preview">
        <div class="preview-form">
          <n-form label-width="100px" size="small">
            <n-form-item label="可执行路径">
              <n-input v-model:value="previewForm.executable" placeholder="/usr/bin/mvn 或 ./mvnw" />
            </n-form-item>
            <n-form-item label="工作目录">
              <n-input v-model:value="previewForm.workingDir" placeholder="/path/to/project" />
            </n-form-item>
            <n-form-item label="Goals">
              <n-input v-model:value="previewForm.goals" placeholder="clean install" />
            </n-form-item>
            <n-form-item label="额外参数">
              <n-input v-model:value="previewForm.extraArgs" placeholder="-DskipTests -Pprod" />
            </n-form-item>
            <n-form-item label="本地仓库">
              <n-input v-model:value="previewForm.localRepository" placeholder="留空用默认" />
            </n-form-item>
            <n-form-item>
              <n-button type="primary" :loading="previewing" @click="onPreview">
                生成命令预览
              </n-button>
            </n-form-item>
          </n-form>
          <div v-if="previewResult" class="preview-result">
            <div class="preview-head">完整命令：</div>
            <pre class="preview-pre">{{ previewResult }}</pre>
          </div>
        </div>
      </n-collapse-item>
    </n-collapse>

    <!-- Raw version output (collapsible) -->
    <n-collapse v-if="executables.some((e) => e.rawVersion)" class="raw-collapse">
      <n-collapse-item title="原始 mvn -v 输出（排查版本差异）" name="raw">
        <div v-for="e in executables.filter((x) => x.rawVersion)" :key="e.id" class="raw-row">
          <div class="raw-head">
            <span class="mono">{{ e.executablePath }}</span>
            <n-tag size="small" :type="e.isValid ? 'success' : 'error'">
              {{ e.isValid ? "有效" : "失效" }}
            </n-tag>
          </div>
          <pre class="raw-pre">{{ e.rawVersion }}</pre>
        </div>
      </n-collapse-item>
    </n-collapse>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, reactive, ref } from "vue";
import { NButton, NIcon, NTag, useMessage } from "naive-ui";
import { TrashOutline, SearchOutline } from "@vicons/ionicons5";
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

const message = useMessage();

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

const tableColumns = [
  {
    title: "状态",
    key: "status",
    width: 90,
    render(row: MavenExecutable) {
      return h(NTag, { size: "small", type: row.isValid ? "success" : "error", bordered: false }, { default: () => row.isValid ? "有效" : "失效" });
    },
  },
  {
    title: "Major",
    key: "major",
    width: 80,
    align: "center" as const,
    render(row: MavenExecutable) {
      return row.majorVersion != null
        ? h("span", { class: "major-badge" }, row.majorVersion)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "完整版本",
    key: "fullVersion",
    minWidth: 120,
    render(row: MavenExecutable) {
      return row.fullVersion
        ? h("span", null, row.fullVersion)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "来源",
    key: "source",
    width: 120,
    render(row: MavenExecutable) {
      return h(NTag, { size: "small", type: "info", bordered: true }, { default: () => sourceLabel(row.source) });
    },
  },
  {
    title: "所属项目",
    key: "projectPath",
    minWidth: 200,
    ellipsis: { tooltip: true },
    render(row: MavenExecutable) {
      return row.projectPath
        ? h("span", { class: "mono" }, row.projectPath)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "可执行路径",
    key: "executablePath",
    minWidth: 280,
    ellipsis: { tooltip: true },
    render(row: MavenExecutable) {
      return h("span", { class: "mono" }, row.executablePath);
    },
  },
  {
    title: "最近校验",
    key: "lastChecked",
    width: 170,
    render(row: MavenExecutable) {
      return row.lastChecked
        ? h("span", { class: "muted" }, formatTime(row.lastChecked))
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 180,
    fixed: "right" as const,
    render(row: MavenExecutable) {
      return h("div", { style: "display:flex;gap:4px" }, [
        h(NButton, {
          size: "small",
          loading: validatingId.value === row.id,
          onClick: () => onValidate(row),
        }, { default: () => "复检" }),
        h(NButton, {
          size: "small",
          type: "error",
          dashed: true,
          onClick: () => onRemove(row),
        }, { default: () => "删除" }),
      ]);
    },
  },
];

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
    message.error("加载 Maven 列表失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

async function onDetectByPicker() {
  detecting.value = true;
  try {
    const resolved = await detectMavenByPicker();
    if (!resolved) return; // 用户取消
    message.success(
      `检测成功：${resolved.executable.fullVersion ?? "?"}（${
        resolved.usesWrapper ? "Wrapper" : sourceLabel(resolved.executable.source)
      }）`,
    );
    await reload();
  } catch (e) {
    // MavenNotFound 等可行动错误在此展示后端给出的提示。
    message.error("检测 Maven 失败：" + errMsg(e));
  } finally {
    detecting.value = false;
  }
}

async function onPrune() {
  pruning.value = true;
  try {
    const n = await pruneInvalidMaven();
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

async function onValidate(row: MavenExecutable) {
  if (row.id == null) return;
  validatingId.value = row.id;
  try {
    const updated = await validateMavenExecutable(row.id);
    const idx = executables.value.findIndex((e) => e.id === row.id);
    if (idx >= 0) executables.value[idx] = updated;
    message.success(
      updated.isValid
        ? `复检通过：major=${updated.majorVersion ?? "?"}`
        : `复检失败，已标记失效`,
    );
  } catch (e) {
    message.error("复检失败：" + errMsg(e));
  } finally {
    validatingId.value = null;
  }
}

async function onRemove(row: MavenExecutable) {
  if (row.id == null) return;
  try {
    await removeMavenExecutable(row.id);
    executables.value = executables.value.filter((e) => e.id !== row.id);
    message.success("已删除");
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
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
    message.error("生成预览失败：" + errMsg(e));
  } finally {
    previewing.value = false;
  }
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
  gap: var(--gw-space-2);
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
.repo-info {
  margin-bottom: 12px;
}
.repo-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
  margin-left: 8px;
}
.summary {
  display: flex;
  gap: 20px;
  align-items: center;
  margin-bottom: 12px;
  font-size: 14px;
  color: var(--gw-text);
}
.summary-item b {
  color: var(--gw-accent);
  margin: 0 2px;
}
.summary-item.valid b {
  color: var(--gw-success);
}
.summary-item.invalid b {
  color: var(--gw-danger);
}
.summary-hint {
  color: var(--gw-text-dim);
  font-size: 12px;
}
.major-badge {
  font-weight: 600;
  color: var(--gw-accent);
}
.muted {
  color: var(--gw-text-dim);
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
  color: var(--gw-text-dim);
  margin-bottom: 4px;
}
.preview-pre {
  background: var(--gw-bg-hover);
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
  gap: var(--gw-space-2);
  align-items: center;
  margin-bottom: 4px;
}
.raw-pre {
  background: var(--gw-bg-hover);
  padding: 8px 12px;
  border-radius: 4px;
  font-size: 12px;
  white-space: pre-wrap;
  margin: 0;
  max-height: 160px;
  overflow: auto;
}
</style>
