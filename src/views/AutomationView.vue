<template>
  <div class="automation-view">
    <div class="automation-header">
      <h2 class="automation-title">自动化（脚本动作 / 定时任务 / 模板）</h2>
      <n-button size="small" :loading="loading" @click="loadAll">
        <template #icon><n-icon><RefreshOutline /></n-icon></template>
        刷新
      </n-button>
    </div>

    <n-spin :show="loading">
      <div class="automation-body">
        <n-tabs type="line" v-model:value="activeTab">
          <!-- ── 脚本动作 ──────────────────────────────────── -->
          <n-tab-pane name="actions" tab="脚本动作">
            <div class="tab-toolbar">
              <n-button size="small" type="primary" ghost @click="openActionEditor()">新建动作</n-button>
            </div>
            <div v-for="a in actions" :key="a.id" class="row">
              <n-tag size="small" :bordered="false">{{ a.scope === "repo" ? "仓库" : "工作区" }}</n-tag>
              <span class="row-main mono">{{ a.name }}</span>
              <span class="row-dim mono">{{ a.command }}</span>
              <n-button size="tiny" type="primary" ghost @click="runAction(a)">运行</n-button>
              <n-button size="tiny" @click="openActionEditor(a)">编辑</n-button>
              <n-button size="tiny" type="error" quaternary @click="removeAction(a)">删除</n-button>
            </div>
            <n-empty v-if="actions.length === 0" description="尚未注册脚本动作" class="tab-empty" />
          </n-tab-pane>

          <!-- ── 定时任务 ──────────────────────────────────── -->
          <n-tab-pane name="schedules" tab="定时任务">
            <div class="tab-toolbar">
              <n-button size="small" type="primary" ghost @click="openScheduleEditor()">新建定时任务</n-button>
            </div>
            <div v-for="t in schedules" :key="t.id" class="row">
              <n-tag size="small" :type="t.enabled ? 'success' : 'default'" :bordered="false">
                {{ t.enabled ? "启用" : "已暂停" }}
              </n-tag>
              <span class="row-main">{{ t.name }}</span>
              <span class="row-dim">
                {{ t.kind === "pipeline" ? "Pipeline" : "脚本" }} ·
                {{ t.scheduleKind === "daily" ? `每天 ${t.dailyTime}` : `每 ${t.intervalMinutes} 分钟` }}
              </span>
              <span class="row-dim">下次：{{ formatTime(t.nextRun) }}</span>
              <n-button size="tiny" @click="toggleSchedule(t)">{{ t.enabled ? "暂停" : "恢复" }}</n-button>
              <n-button size="tiny" type="error" quaternary @click="removeSchedule(t)">删除</n-button>
            </div>
            <n-empty v-if="schedules.length === 0" description="尚未创建定时任务" class="tab-empty" />
          </n-tab-pane>

          <!-- ── 模板导入 / 导出 ────────────────────────────── -->
          <n-tab-pane name="templates" tab="Pipeline 模板">
            <div class="tab-toolbar">
              <n-button size="small" @click="importTemplate">导入 JSON…</n-button>
            </div>
            <div v-for="t in templates" :key="t.id" class="row">
              <span class="row-main mono">{{ t.name }}</span>
              <span class="row-dim">{{ t.steps?.length ?? 0 }} 步</span>
              <n-button size="tiny" @click="exportTemplate(t)">导出 JSON…</n-button>
            </div>
            <n-empty v-if="templates.length === 0" description="无 Pipeline 模板" class="tab-empty" />
          </n-tab-pane>
        </n-tabs>
      </div>
    </n-spin>

    <!-- 动作编辑器 -->
    <n-modal v-model:show="actionEditor.show" preset="card" title="脚本动作" style="width: 560px">
      <div class="editor-form">
        <span class="editor-label">名称</span>
        <n-input v-model:value="actionEditor.name" size="small" placeholder="如：运行单元测试" />
        <span class="editor-label">命令（跨平台 shell 语义）</span>
        <n-input v-model:value="actionEditor.command" type="textarea" :rows="3" placeholder="如：cargo test --lib" class="mono" />
        <span class="editor-label">作用域</span>
        <n-radio-group v-model:value="actionEditor.scope">
          <n-radio value="repo">仓库（cwd = 仓库根）</n-radio>
          <n-radio value="workspace">工作区（cwd = 工作区根）</n-radio>
        </n-radio-group>
        <span class="editor-label">超时（秒）</span>
        <n-input-number v-model:value="actionEditor.timeoutSecs" size="small" :min="5" :max="3600" />
      </div>
      <template #footer>
        <n-button @click="actionEditor.show = false">取消</n-button>
        <n-button type="primary" @click="saveAction">保存</n-button>
      </template>
    </n-modal>

    <!-- 定时任务编辑器 -->
    <n-modal v-model:show="scheduleEditor.show" preset="card" title="定时任务" style="width: 560px">
      <div class="editor-form">
        <span class="editor-label">名称</span>
        <n-input v-model:value="scheduleEditor.name" size="small" placeholder="如：每晚同步" />
        <span class="editor-label">类型</span>
        <n-radio-group v-model:value="scheduleEditor.kind">
          <n-radio value="script_action">脚本动作</n-radio>
          <n-radio value="pipeline">Pipeline 模板</n-radio>
        </n-radio-group>
        <span class="editor-label">目标</span>
        <n-select
          v-model:value="scheduleEditor.targetId"
          size="small"
          :options="scheduleTargetOptions"
          placeholder="选择目标"
        />
        <span class="editor-label">调度</span>
        <n-radio-group v-model:value="scheduleEditor.scheduleKind">
          <n-radio value="interval">固定间隔</n-radio>
          <n-radio value="daily">每天固定时间</n-radio>
        </n-radio-group>
        <n-input-number
          v-if="scheduleEditor.scheduleKind === 'interval'"
          v-model:value="scheduleEditor.intervalMinutes"
          size="small"
          :min="5"
          :max="10080"
        >
          <template #suffix>分钟</template>
        </n-input-number>
        <n-input
          v-if="scheduleEditor.scheduleKind === 'daily'"
          v-model:value="scheduleEditor.dailyTime"
          size="small"
          placeholder="HH:MM（本地时区）"
        />
        <template v-if="scheduleEditor.kind === 'pipeline'">
          <span class="editor-label">仓库选择 JSON（可选，留空 = 空选择）</span>
          <n-input v-model:value="scheduleEditor.payload" type="textarea" :rows="3" class="mono" placeholder='[]' />
        </template>
      </div>
      <template #footer>
        <n-button @click="scheduleEditor.show = false">取消</n-button>
        <n-button type="primary" @click="saveSchedule">保存</n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useMessage } from "naive-ui";
import { RefreshOutline } from "@vicons/ionicons5";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import {
  deletePluginAction,
  deleteScheduledTask,
  exportPipelineTemplate,
  importPipelineTemplate,
  listPluginActions,
  listScheduledTasks,
  runPluginAction,
  savePluginAction,
  saveScheduledTask,
  setScheduledTaskEnabled,
  type PluginAction,
  type ScheduledTask,
} from "@/api/automation";
import { listPipelineTemplates } from "@/api/pipeline";
import type { Pipeline } from "@/types/pipeline";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";
import { errMsg } from "@/utils/error";

const message = useMessage();
const workspaceStore = useWorkspaceStore();
const repoStore = useRepositoryStore();

const loading = ref(false);
const activeTab = ref<"actions" | "schedules" | "templates">("actions");
const actions = ref<PluginAction[]>([]);
const schedules = ref<ScheduledTask[]>([]);
const templates = ref<Pipeline[]>([]);

const actionEditor = ref<{
  show: boolean;
  id: number;
  name: string;
  command: string;
  scope: "repo" | "workspace";
  timeoutSecs: number;
}>({ show: false, id: 0, name: "", command: "", scope: "repo", timeoutSecs: 120 });

const scheduleEditor = ref<{
  show: boolean;
  id: number;
  name: string;
  kind: "script_action" | "pipeline";
  targetId: string | null;
  scheduleKind: "interval" | "daily";
  intervalMinutes: number;
  dailyTime: string | null;
  payload: string;
}>({
  show: false,
  id: 0,
  name: "",
  kind: "script_action",
  targetId: null,
  scheduleKind: "interval",
  intervalMinutes: 30,
  dailyTime: null,
  payload: "",
});

const scheduleTargetOptions = ref<{ label: string; value: string }[]>([]);

onMounted(loadAll);

async function loadAll() {
  loading.value = true;
  try {
    actions.value = await listPluginActions();
    schedules.value = await listScheduledTasks();
    templates.value = await listPipelineTemplates();
    scheduleTargetOptions.value = [
      ...actions.value.map((a) => ({ label: `[脚本] ${a.name}`, value: String(a.id) })),
      ...templates.value.map((t) => ({ label: `[Pipeline] ${t.name}`, value: t.id })),
    ];
  } catch (e) {
    message.error("加载自动化配置失败: " + errMsg(e));
  } finally {
    loading.value = false;
  }
}

// ── 脚本动作 ────────────────────────────────────────────────

function openActionEditor(a?: PluginAction) {
  actionEditor.value = {
    show: true,
    id: a?.id ?? 0,
    name: a?.name ?? "",
    command: a?.command ?? "",
    scope: a?.scope ?? "repo",
    timeoutSecs: a?.timeoutSecs ?? 120,
  };
}

async function saveAction() {
  try {
    await savePluginAction({
      id: actionEditor.value.id,
      name: actionEditor.value.name,
      command: actionEditor.value.command,
      scope: actionEditor.value.scope,
      timeoutSecs: actionEditor.value.timeoutSecs,
    });
    message.success("动作已保存");
    actionEditor.value.show = false;
    await loadAll();
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  }
}

function resolveCwd(scope: "repo" | "workspace"): string {
  if (scope === "workspace") {
    return workspaceStore.currentWorkspace?.path ?? "";
  }
  return repoStore.currentRepoPath || workspaceStore.currentWorkspace?.path || "";
}

async function runAction(a: PluginAction) {
  const cwd = resolveCwd(a.scope);
  if (!cwd) {
    message.warning("当前没有可用的仓库 / 工作区上下文");
    return;
  }
  try {
    const out = await runPluginAction(cwd, a);
    message.success(`已运行 ${a.name}${out ? "：" + out.trim().slice(0, 300) : "（无输出）"}`);
  } catch (e) {
    message.error(`运行 ${a.name} 失败: ` + errMsg(e));
  }
}

async function removeAction(a: PluginAction) {
  try {
    await deletePluginAction(a.id);
    message.success("动作已删除");
    await loadAll();
  } catch (e) {
    message.error("删除失败: " + errMsg(e));
  }
}

// ── 定时任务 ────────────────────────────────────────────────

function openScheduleEditor() {
  scheduleEditor.value = {
    ...scheduleEditor.value,
    show: true,
    id: 0,
    name: "",
    kind: "script_action",
    targetId: null,
    scheduleKind: "interval",
    intervalMinutes: 30,
    dailyTime: null,
    payload: "",
  };
}

async function saveSchedule() {
  if (!scheduleEditor.value.targetId) {
    message.warning("请选择目标");
    return;
  }
  try {
    await saveScheduledTask({
      id: scheduleEditor.value.id,
      name: scheduleEditor.value.name,
      kind: scheduleEditor.value.kind,
      targetId: scheduleEditor.value.targetId,
      scheduleKind: scheduleEditor.value.scheduleKind,
      intervalMinutes:
        scheduleEditor.value.scheduleKind === "interval"
          ? scheduleEditor.value.intervalMinutes
          : null,
      dailyTime:
        scheduleEditor.value.scheduleKind === "daily"
          ? scheduleEditor.value.dailyTime
          : null,
      payload: scheduleEditor.value.kind === "pipeline" ? scheduleEditor.value.payload || null : null,
      enabled: true,
    });
    message.success("定时任务已保存");
    scheduleEditor.value.show = false;
    await loadAll();
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  }
}

async function toggleSchedule(t: ScheduledTask) {
  try {
    await setScheduledTaskEnabled(t.id, !t.enabled);
    await loadAll();
  } catch (e) {
    message.error("切换失败: " + errMsg(e));
  }
}

async function removeSchedule(t: ScheduledTask) {
  try {
    await deleteScheduledTask(t.id);
    message.success("定时任务已删除");
    await loadAll();
  } catch (e) {
    message.error("删除失败: " + errMsg(e));
  }
}

// ── 模板导入 / 导出 ─────────────────────────────────────────

async function exportTemplate(t: Pipeline) {
  try {
    const path = await saveFileDialog({
      defaultPath: `${t.name || "pipeline"}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return;
    await exportPipelineTemplate(t.id, path);
    message.success("模板已导出：" + path);
  } catch (e) {
    message.error("导出失败: " + errMsg(e));
  }
}

async function importTemplate() {
  try {
    const path = await openFileDialog({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path || Array.isArray(path)) return;
    const saved = await importPipelineTemplate(path);
    message.success(`模板已导入：${saved.name}`);
    await loadAll();
  } catch (e) {
    message.error("导入失败: " + errMsg(e));
  }
}

function formatTime(value: string): string {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString();
}
</script>

<style scoped>
.automation-view {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.automation-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 16px;
  border-bottom: 1px solid var(--gw-border);
  background: var(--gw-bg-panel);
}

.automation-title {
  font-size: 15px;
  font-weight: 600;
}

.automation-body {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 16px;
  background: var(--gw-bg-panel);
}

.tab-toolbar {
  display: flex;
  gap: var(--gw-space-2);
  margin-bottom: var(--gw-space-3);
}

.row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
  padding: 6px 0;
  border-bottom: 1px solid var(--gw-border);
  font-size: 13px;
}

.row-main {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.row-dim {
  color: var(--gw-text-dim);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 300px;
}

.mono {
  font-family: var(--gw-font-mono);
}

.editor-form {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.editor-label {
  font-size: 12px;
  color: var(--gw-text-dim);
}

.tab-empty {
  margin-top: var(--gw-space-6);
}
</style>
