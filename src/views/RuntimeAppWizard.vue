<template>
  <div class="runtime-wizard">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <span class="page-title">{{ isEdit ? `编辑应用 · ${form.name}` : "新建 Runtime 应用" }}</span>
      </div>
      <div class="toolbar-right">
        <n-button @click="goBack">取消</n-button>
        <n-button type="primary" :loading="saving" @click="onSave">
          <template #icon><n-icon><CheckmarkOutline /></n-icon></template>
          {{ isEdit ? "保存修改" : "创建应用" }}
        </n-button>
      </div>
    </div>

    <n-form
      ref="formRef"
      :model="form"
      :rules="rules"
      label-placement="left"
      label-width="150"
      class="wizard-form"
    >
      <n-form-item label="名称" path="name">
        <n-input
          v-model:value="form.name"
          :disabled="isEdit"
          placeholder="例如 boot-app（运行时唯一标识）"
          style="max-width: 320px"
        />
      </n-form-item>

      <n-form-item label="Maven 项目" path="project">
        <div class="project-field">
          <n-select
            v-model:value="form.project"
            :options="store.projects.map(p => ({ label: projectLabel(p), value: p.path }))"
            placeholder="选择 workspace 内的 Maven 项目"
            filterable
            :loading="store.loading"
            style="width: 100%; max-width: 560px"
            @update:value="onProjectChange"
          />
          <!-- R-14 空态引导：索引为空（未解析依赖 / 仓库无 .git 标记）时给出
               明确动作，而不是裸 no data。 -->
          <n-alert
            v-if="store.workspaceId && store.projects.length === 0 && !store.loading"
            type="info"
            :show-icon="true"
            :bordered="false"
            class="projects-empty-alert"
          >
            尚未发现 Maven 项目。点击「解析依赖」建立索引（仅识别带 .git 标记的仓库内的 pom.xml；
            也可在 Dashboard 执行，长任务进度见任务面板）。
          </n-alert>
          <n-button
            v-if="store.workspaceId && store.projects.length === 0"
            size="small"
            type="primary"
            dashed
            :loading="resolving"
            @click="onResolve"
          >
            <template #icon><n-icon><RefreshOutline /></n-icon></template>
            解析依赖
          </n-button>
        </div>
      </n-form-item>

      <n-form-item label="Main Class">
        <div class="main-class-row">
          <n-input
            v-model:value="form.mainClass"
            placeholder="例如 com.example.Application（留空由 R-06 推断）"
            style="flex: 1"
          />
          <n-button :loading="detecting" @click="onDetectMainClass">
            <template #icon><n-icon><SparklesOutline /></n-icon></template>
            自动检测
          </n-button>
        </div>
      </n-form-item>

      <n-form-item label="JDK">
        <n-select
          v-model:value="form.jdk"
          :options="jdkOptions"
          placeholder="默认（系统 JAVA_HOME）"
          clearable
          style="width: 100%; max-width: 560px"
        />
      </n-form-item>

      <n-form-item label="Profile">
        <n-input
          v-model:value="form.profile"
          placeholder="例如 dev（等价 --spring.profiles.active=dev）"
          style="max-width: 320px"
        />
      </n-form-item>

      <n-form-item label="启动预设">
        <div class="preset-field">
          <n-select
            :options="presetOptions"
            placeholder="选择预设模板，一键填充 VM Options（可选）"
            clearable
            style="width: 100%; max-width: 560px"
            @update:value="applyPreset"
          />
          <div v-if="appliedPresetHint" class="field-hint">{{ appliedPresetHint }}</div>
        </div>
      </n-form-item>

      <n-form-item label="VM Options">
        <n-input
          v-model:value="vmOptionsText"
          type="textarea"
          :rows="2"
          placeholder="每行一个，例如 -Xmx1g / -Dserver.port=8080"
          style="width: 100%; max-width: 560px"
        />
      </n-form-item>

      <n-form-item label="Program Arguments">
        <n-input
          v-model:value="programArgsText"
          type="textarea"
          :rows="2"
          placeholder="每行一个，例如 --server.port=8080"
          style="width: 100%; max-width: 560px"
        />
      </n-form-item>

      <n-form-item label="Pre-Build 脚本">
        <div class="script-field">
          <n-input
            v-model:value="preBuildScriptText"
            type="textarea"
            :rows="3"
            placeholder="构建前执行的 shell 脚本（首次执行必须确认，默认禁止自动执行）"
            style="width: 100%; max-width: 560px"
          />
          <div class="field-hint warn-hint">
            ⚠ 脚本在构建前于 workspace 根目录执行；首次执行需在 Dashboard 确认（确认状态持久化，内容变更后需重新确认）。
          </div>
        </div>
      </n-form-item>

      <n-form-item label="Post-Build 脚本">
        <div class="script-field">
          <n-input
            v-model:value="postBuildScriptText"
            type="textarea"
            :rows="3"
            placeholder="构建成功后执行的 shell 脚本（同上确认规则）"
            style="width: 100%; max-width: 560px"
          />
        </div>
      </n-form-item>

      <n-form-item label="环境变量">
        <div class="env-editor">
          <n-data-table
            :columns="envColumns"
            :data="envRows"
            size="small"
            :bordered="true"
            max-height="300"
          />
          <n-button size="small" @click="addEnvRow">
            <template #icon><n-icon><AddOutline /></n-icon></template>
            添加变量
          </n-button>
          <div class="field-hint">
            敏感 key（PASSWORD / TOKEN / SECRET / PRIVATE_KEY / API_KEY 等）在 UI /
            日志 / IPC 三处统一掩码（全局约束 §4）。
          </div>
        </div>
      </n-form-item>

      <n-form-item label="构建引擎">
        <n-select
          v-model:value="form.buildEngine"
          :options="buildEngineOptions"
          style="width: 200px"
        />
        <div class="field-hint">mvnd 未安装 / 异常时自动回退普通 Maven（R-18）。</div>
      </n-form-item>

      <n-form-item label="健康检查">
        <n-space vertical style="width: 100%" :size="8">
          <n-space align="center">
            <n-checkbox v-model:checked="healthEnabled">启用探针（R-16）</n-checkbox>
            <n-select
              v-model:value="healthForm.kind"
              :options="[
                { label: 'Auto（Actuator 优先，回退 TCP）', value: 'auto' },
                { label: 'Actuator（/actuator/health）', value: 'actuator' },
                { label: 'HTTP GET', value: 'http' },
                { label: 'TCP 端口', value: 'tcp' },
                { label: '端口（本机 127.0.0.1）', value: 'port' },
              ]"
              :disabled="!healthEnabled"
              style="width: 280px"
              size="small"
            />
          </n-space>
          <n-space align="center" v-if="healthEnabled">
            <n-input-number
              v-model:value="healthForm.port"
              placeholder="端口（缺省用探测端口）"
              :min="1"
              :max="65535"
              :show-button="false"
              style="width: 200px"
              size="small"
            />
            <n-input
              v-model:value="healthForm.path"
              placeholder="路径，缺省 /actuator/health"
              :disabled="healthForm.kind === 'port' || healthForm.kind === 'tcp'"
              style="width: 220px"
              size="small"
            />
            <n-input-number
              v-model:value="healthForm.intervalMs"
              placeholder="间隔 ms"
              :min="500"
              :show-button="false"
              style="width: 120px"
              size="small"
            />
          </n-space>
          <div class="field-hint">
            启用后 Running 状态按间隔探测：Starting → Healthy / Unhealthy；无配置时保持
            「启动即 Up」的生命周期推导语义。
          </div>
        </n-space>
      </n-form-item>
    </n-form>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NButton, NIcon, NInput, NTag, useMessage } from "naive-ui";
import {
  AddOutline,
  CheckmarkOutline,
  RefreshOutline,
  SparklesOutline,
} from "@vicons/ionicons5";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import { listJdks } from "@/api/jdk";
import { detectSpringBoot } from "@/api/springBoot";
import { LAUNCH_PRESETS } from "@/config/launchPresets";
import type { JdkInstallation } from "@/types/jdk";
import type { MavenProjectNode, RuntimeScope } from "@/types/maven";
import type { RuntimeApplicationConfig } from "@/types/runtime";
import { errMsg } from "@/utils/error";

const message = useMessage();
const route = useRoute();
const router = useRouter();
// R-14 修复：向导此前未初始化 workspace（其他视图均走 useRuntimeWorkspace），
// 直接进入向导时 workspaceId 为空 → 项目列表恒为 no data。
const { workspaceStore, store, ensureWorkspace } =
  useRuntimeWorkspace();

const isEdit = computed(() => !!route.query.edit);
const saving = ref(false);
const detecting = ref(false);
const resolving = ref(false);
const formRef = ref<InstanceType<typeof import("naive-ui").NForm>>();

const form = reactive({
  name: "",
  project: "",
  mainClass: "",
  jdk: "",
  profile: "",
  buildEngine: "maven",
});

/** 编辑模式保留原有 Scope（向导不负责 Scope 编辑；Scope 视图专属）。 */
const originalScope = ref<RuntimeScope>({ mode: "auto" });

const vmOptionsText = ref("");
const programArgsText = ref("");

/** F-04：启动预设（一键填充 VM Options；覆盖式填充，选择即生效）。 */
const presetOptions = LAUNCH_PRESETS.map((p) => ({
  label: p.label,
  value: p.id,
}));
const appliedPresetHint = ref("");

function applyPreset(id: string | null) {
  if (!id) {
    appliedPresetHint.value = "";
    return;
  }
  const preset = LAUNCH_PRESETS.find((p) => p.id === id);
  if (!preset) return;
  vmOptionsText.value = preset.vmOptions.join("\n");
  appliedPresetHint.value = preset.description;
  message.success(`已应用预设「${preset.label}」`);
}
/** R-14 §75：Pre/Post Build Script（首次执行必须确认）。 */
const preBuildScriptText = ref("");
const postBuildScriptText = ref("");

// R-18 §20：构建引擎（maven / mvnd；mvnd 缺失时后端自动回退）。
const buildEngineOptions = [
  { label: "Maven", value: "maven" },
  { label: "Maven Daemon (mvnd)", value: "mvnd" },
];

// R-16 §41：健康检查（未启用 → healthCheck = null，保持生命周期推导语义）。
const healthEnabled = ref(false);
const healthForm = reactive({
  kind: "auto",
  port: null as number | null,
  path: "",
  intervalMs: null as number | null,
});

interface EnvRow {
  key: string;
  value: string;
}
const envRows = ref<EnvRow[]>([]);

const envColumns = [
  {
    title: "Key",
    key: "key",
    minWidth: 180,
    render: (row: EnvRow) =>
      h(NInput, {
        value: row.key,
        size: "small",
        placeholder: "KEY",
        onUpdateValue: (v: string) => {
          row.key = v;
        },
      }),
  },
  {
    title: "Value",
    key: "value",
    minWidth: 240,
    render: (row: EnvRow) =>
      h(NInput, {
        value: row.value,
        size: "small",
        type: isSensitiveKey(row.key) ? "password" : "text",
        showPasswordOn: "click",
        placeholder: isSensitiveKey(row.key) ? "敏感值（保存后掩码显示）" : "value",
        onUpdateValue: (v: string) => {
          row.value = v;
        },
      }),
  },
  {
    title: "敏感",
    key: "sensitive",
    width: 80,
    align: "center" as const,
    render: (row: EnvRow) =>
      isSensitiveKey(row.key)
        ? h(NTag, { size: "small", type: "error", bordered: false }, { default: () => "敏感" })
        : h("span", { class: "muted" }, "—"),
  },
  {
    title: "",
    key: "actions",
    width: 60,
    align: "center" as const,
    render: (_row: EnvRow, index: number) =>
      h(
        NButton,
        {
          size: "small",
          text: true,
          type: "error",
          onClick: () => removeEnvRow(index),
        },
        { default: () => "删除" },
      ),
  },
];

const rules = {
  name: [{ required: true, message: "请输入应用名称", trigger: "blur" }],
  project: [{ required: true, message: "请选择 Maven 项目", trigger: "change" }],
};

// ------------------------------------------------------------------
// JDK 下拉（R-04 注册表；spec = 前导 major 数字或 home path）
// ------------------------------------------------------------------

const jdks = ref<JdkInstallation[]>([]);

const jdkOptions = computed(() => {
  const seen = new Set<string>();
  const opts: { value: string; label: string }[] = [];
  for (const j of jdks.value) {
    if (!j.isValid) continue;
    const key = j.majorVersion != null ? String(j.majorVersion) : j.homePath;
    if (seen.has(key)) continue;
    seen.add(key);
    opts.push({
      value: key,
      label: `JDK ${j.majorVersion ?? "?"} (${j.vendor ?? "unknown"}) — ${j.homePath}`,
    });
  }
  return opts;
});

// ------------------------------------------------------------------
// 初始化 / 加载
// ------------------------------------------------------------------

function projectLabel(p: MavenProjectNode): string {
  return `${p.coordinates.artifactId}  (${p.path})`;
}

function toConfig(): RuntimeApplicationConfig {
  const env: Record<string, string> = {};
  for (const row of envRows.value) {
    const key = row.key.trim();
    if (key) env[key] = row.value;
  }
  return {
    schemaVersion: 1,
    name: form.name.trim(),
    project: form.project,
    mainClass: form.mainClass.trim() || null,
    jdk: form.jdk || null,
    profile: form.profile.trim() || null,
    vmOptions: vmOptionsText.value
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean),
    programArguments: programArgsText.value
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean),
    environment: env,
    runtimeEnvironment: {},
    buildEngine: form.buildEngine || null,
    scope: originalScope.value,
    preBuildScript: preBuildScriptText.value.trim() || null,
    postBuildScript: postBuildScriptText.value.trim() || null,
    healthCheck: healthConfig(),
  };
}

/** R-16：把健康检查表单收敛为 HealthCheckConfig；未启用或全空 → null。 */
function healthConfig(): RuntimeApplicationConfig["healthCheck"] {
  if (!healthEnabled.value) return null;
  const config = {
    kind: healthForm.kind as "auto" | "port" | "http" | "tcp" | "actuator",
    host: null,
    port: healthForm.port,
    path: healthForm.path.trim() || null,
    intervalMs: healthForm.intervalMs,
    timeoutMs: null,
    healthyAfter: null,
    unhealthyAfter: null,
  };
  if (config.port == null && config.path == null && config.intervalMs == null && config.kind === "auto") {
    // 全缺省 = 没有有效配置；视为未启用，避免保存一份等价默认的 JSON。
    return null;
  }
  return config;
}

function fillForm(config: RuntimeApplicationConfig) {
  form.name = config.name;
  form.project = config.project;
  form.mainClass = config.mainClass ?? "";
  form.jdk = config.jdk ?? "";
  form.profile = config.profile ?? "";
  form.buildEngine = config.buildEngine ?? "maven";
  originalScope.value = config.scope ?? { mode: "auto" };
  vmOptionsText.value = config.vmOptions.join("\n");
  programArgsText.value = config.programArguments.join("\n");
  preBuildScriptText.value = config.preBuildScript ?? "";
  postBuildScriptText.value = config.postBuildScript ?? "";
  const hc = config.healthCheck;
  healthEnabled.value = !!hc;
  if (hc) {
    healthForm.kind = hc.kind ?? "auto";
    healthForm.port = hc.port ?? null;
    healthForm.path = hc.path ?? "";
    healthForm.intervalMs = hc.intervalMs ?? null;
  } else {
    healthForm.kind = "auto";
    healthForm.port = null;
    healthForm.path = "";
    healthForm.intervalMs = null;
  }
  envRows.value = Object.entries(config.environment).map(([key, value]) => ({
    key,
    value,
  }));
}

function isSensitiveKey(key: string): boolean {
  return /(PASSWORD|TOKEN|SECRET|PRIVATE_KEY|API_KEY)/i.test(key);
}

function addEnvRow() {
  envRows.value.push({ key: "", value: "" });
}

function removeEnvRow(index: number) {
  envRows.value.splice(index, 1);
}

async function onProjectChange() {
  // 项目变化时尝试用 R-06 检测结果预填 Main Class（仅当用户未手填）。
  if (form.mainClass.trim()) return;
  await onDetectMainClass();
}

async function onDetectMainClass() {
  if (!form.project) {
    message.warning("请先选择 Maven 项目");
    return;
  }
  if (!store.workspaceId) {
    message.warning("未选择 workspace");
    return;
  }
  const ws = workspaceStore.workspaces.find((w) => w.id === store.workspaceId);
  if (!ws) return;
  detecting.value = true;
  try {
    const result = await detectSpringBoot(ws.path);
    // 匹配当前项目。R-02 索引里的 path 统一是正斜杠，而 Rust 检测返回的
    // projectPath 在 Windows 上是反斜杠——比较前两侧都归一化（AGENTS.md
    // 平台规范 §1，F-05 修复：hussar-base-web 曾因分隔符不一致匹配失败）。
    const norm = (s: string) => s.replace(/\\/g, "/");
    const needle = norm(form.project);
    const project = result.projects.find((p) => {
      const pomPath = norm(p.projectPath);
      return (
        needle === pomPath ||
        pomPath.endsWith(`/${needle}`) ||
        needle === p.module ||
        needle.endsWith(`/${p.module}`)
      );
    });
    const candidate = project?.defaultMainClass || project?.candidates?.[0]?.className;
    if (candidate) {
      form.mainClass = candidate;
      message.success(`已预填 Main Class：${candidate}`);
    } else {
      message.info("该项目未检测到 Spring Boot Main Class，可手动填写");
    }
  } catch (e) {
    message.error("检测失败：" + errMsg(e));
  } finally {
    detecting.value = false;
  }
}

async function onSave() {
  try {
    await formRef.value?.validate();
  } catch {
    return;
  }
  if (!store.workspaceId) {
    message.warning("未选择 workspace");
    return;
  }
  saving.value = true;
  try {
    await store.saveConfig(toConfig());
    message.success(isEdit.value ? "配置已保存" : "应用已创建");
    router.push({ name: "runtime-dashboard" });
  } catch (e) {
    message.error("保存失败：" + errMsg(e));
  } finally {
    saving.value = false;
  }
}

async function onResolve() {
  if (!store.workspaceId) return;
  resolving.value = true;
  try {
    await store.resolveDependencies();
    message.success("依赖解析任务已提交，完成后项目列表自动刷新");
  } catch (e) {
    message.error("解析失败：" + errMsg(e));
  } finally {
    resolving.value = false;
  }
}

function goBack() {
  router.push({ name: "runtime-dashboard" });
}

onMounted(async () => {
  // 先确保 workspace 就绪（含事件订阅），再加载 JDK / 编辑配置。
  await ensureWorkspace();
  try {
    jdks.value = await listJdks();
  } catch (e) {
    console.error("R-13: load JDKs failed:", e);
  }
  if (isEdit.value) {
    const name = String(route.query.edit ?? "");
    try {
      const config = await store.loadConfigDetail(name);
      fillForm(config);
    } catch (e) {
      message.error("加载配置失败：" + errMsg(e));
      goBack();
    }
  }
});
</script>

<style scoped>
.runtime-wizard {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
  overflow-y: auto;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2);
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}
.page-title {
  font-size: 15px;
  font-weight: 600;
}
.wizard-form {
  max-width: 860px;
  padding: 8px 4px;
}
.field-hint {
  font-size: 12px;
  color: var(--gw-text-dim);
  margin-top: 4px;
  width: 100%;
}
.preset-field {
  width: 100%;
  max-width: 560px;
}
.field-hint.warn-hint {
  color: var(--gw-warning);
}
.script-field {
  width: 100%;
}
.project-field {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  align-items: flex-start;
}
.projects-empty-alert {
  width: 100%;
  max-width: 560px;
}
.main-class-row {
  display: flex;
  gap: var(--gw-space-2);
  width: 100%;
}
.env-editor {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  align-items: flex-start;
}
.muted {
  color: var(--gw-text-dim);
}
</style>
