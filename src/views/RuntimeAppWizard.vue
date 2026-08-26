<template>
  <div class="runtime-wizard">
    <!-- Toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <el-button text @click="goBack">
          <el-icon><Back /></el-icon>
          返回
        </el-button>
        <el-select
          v-model="selectedWorkspaceId"
          placeholder="选择工作区"
          style="width: 200px"
          @change="selectWorkspace"
        >
          <el-option
            v-for="ws in workspaceStore.workspaces"
            :key="ws.id"
            :label="ws.name"
            :value="ws.id"
          />
        </el-select>
        <span class="page-title">{{ isEdit ? `编辑应用 · ${form.name}` : "新建 Runtime 应用" }}</span>
      </div>
      <div class="toolbar-right">
        <el-button @click="goBack">取消</el-button>
        <el-button type="primary" :loading="saving" @click="onSave">
          <el-icon><Check /></el-icon>
          {{ isEdit ? "保存修改" : "创建应用" }}
        </el-button>
      </div>
    </div>

    <el-form
      ref="formRef"
      :model="form"
      :rules="rules"
      label-width="150px"
      label-position="left"
      class="wizard-form"
    >
      <el-form-item label="名称" prop="name">
        <el-input
          v-model="form.name"
          :disabled="isEdit"
          placeholder="例如 boot-app（运行时唯一标识）"
          style="max-width: 320px"
        />
      </el-form-item>

      <el-form-item label="Maven 项目" prop="project">
        <div class="project-field">
          <el-select
            v-model="form.project"
            placeholder="选择 workspace 内的 Maven 项目"
            filterable
            :loading="store.loading"
            style="width: 100%; max-width: 560px"
            @change="onProjectChange"
          >
            <el-option
              v-for="p in store.projects"
              :key="p.projectId"
              :label="projectLabel(p)"
              :value="p.path"
            />
          </el-select>
          <!-- R-14 空态引导：索引为空（未解析依赖 / 仓库无 .git 标记）时给出
               明确动作，而不是裸 no data。 -->
          <el-alert
            v-if="store.workspaceId && store.projects.length === 0 && !store.loading"
            type="info"
            :closable="false"
            show-icon
            class="projects-empty-alert"
          >
            <template #title>
              尚未发现 Maven 项目。点击「解析依赖」建立索引（仅识别带 .git 标记的仓库内的 pom.xml；
              也可在 Dashboard 执行，长任务进度见任务面板）。
            </template>
          </el-alert>
          <el-button
            v-if="store.workspaceId && store.projects.length === 0"
            size="small"
            type="primary"
            plain
            :loading="resolving"
            @click="onResolve"
          >
            <el-icon><Refresh /></el-icon>
            解析依赖
          </el-button>
        </div>
      </el-form-item>

      <el-form-item label="Main Class">
        <div class="main-class-row">
          <el-input
            v-model="form.mainClass"
            placeholder="例如 com.example.Application（留空由 R-06 推断）"
            style="flex: 1"
          />
          <el-button :loading="detecting" @click="onDetectMainClass">
            <el-icon><MagicStick /></el-icon>
            自动检测
          </el-button>
        </div>
      </el-form-item>

      <el-form-item label="JDK">
        <el-select
          v-model="form.jdk"
          placeholder="默认（系统 JAVA_HOME）"
          clearable
          style="width: 100%; max-width: 560px"
        >
          <el-option
            v-for="opt in jdkOptions"
            :key="opt.value"
            :label="opt.label"
            :value="opt.value"
          />
        </el-select>
      </el-form-item>

      <el-form-item label="Profile">
        <el-input
          v-model="form.profile"
          placeholder="例如 dev（等价 --spring.profiles.active=dev）"
          style="max-width: 320px"
        />
      </el-form-item>

      <el-form-item label="VM Options">
        <el-input
          v-model="vmOptionsText"
          type="textarea"
          :rows="2"
          placeholder="每行一个，例如 -Xmx1g / -Dserver.port=8080"
          style="width: 100%; max-width: 560px"
        />
      </el-form-item>

      <el-form-item label="Program Arguments">
        <el-input
          v-model="programArgsText"
          type="textarea"
          :rows="2"
          placeholder="每行一个，例如 --server.port=8080"
          style="width: 100%; max-width: 560px"
        />
      </el-form-item>

      <el-form-item label="Pre-Build 脚本">
        <div class="script-field">
          <el-input
            v-model="preBuildScriptText"
            type="textarea"
            :rows="3"
            placeholder="构建前执行的 shell 脚本（首次执行必须确认，默认禁止自动执行）"
            style="width: 100%; max-width: 560px"
          />
          <div class="field-hint warn-hint">
            ⚠ 脚本在构建前于 workspace 根目录执行；首次执行需在 Dashboard 确认（确认状态持久化，内容变更后需重新确认）。
          </div>
        </div>
      </el-form-item>

      <el-form-item label="Post-Build 脚本">
        <div class="script-field">
          <el-input
            v-model="postBuildScriptText"
            type="textarea"
            :rows="3"
            placeholder="构建成功后执行的 shell 脚本（同上确认规则）"
            style="width: 100%; max-width: 560px"
          />
        </div>
      </el-form-item>

      <el-form-item label="环境变量">
        <div class="env-editor">
          <el-table :data="envRows" size="small" empty-text="暂无环境变量">
            <el-table-column label="Key" min-width="180">
              <template #default="{ row }">
                <el-input v-model="row.key" size="small" placeholder="KEY" />
              </template>
            </el-table-column>
            <el-table-column label="Value" min-width="240">
              <template #default="{ row }">
                <el-input
                  v-model="row.value"
                  size="small"
                  :type="isSensitiveKey(row.key) ? 'password' : 'text'"
                  show-password
                  :placeholder="isSensitiveKey(row.key) ? '敏感值（保存后掩码显示）' : 'value'"
                />
              </template>
            </el-table-column>
            <el-table-column label="敏感" width="80" align="center">
              <template #default="{ row }">
                <el-tag v-if="isSensitiveKey(row.key)" size="small" type="danger" effect="plain">
                  敏感
                </el-tag>
                <span v-else class="muted">—</span>
              </template>
            </el-table-column>
            <el-table-column label="" width="60" align="center">
              <template #default="{ $index }">
                <el-button size="small" link type="danger" @click="removeEnvRow($index)">
                  删除
                </el-button>
              </template>
            </el-table-column>
          </el-table>
          <el-button size="small" @click="addEnvRow">
            <el-icon><Plus /></el-icon>
            添加变量
          </el-button>
          <div class="field-hint">
            敏感 key（PASSWORD / TOKEN / SECRET / PRIVATE_KEY / API_KEY 等）在 UI /
            日志 / IPC 三处统一掩码（全局约束 §4）。
          </div>
        </div>
      </el-form-item>

      <el-form-item label="构建引擎">
        <el-select v-model="form.buildEngine" disabled style="width: 200px">
          <el-option label="Maven" value="maven" />
        </el-select>
        <div class="field-hint">mvnd（R-18）/ Gradle（R-22）将在此扩展。</div>
      </el-form-item>
    </el-form>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { Back, Check, Plus, MagicStick, Refresh } from "@element-plus/icons-vue";
import type { FormInstance, FormRules } from "element-plus";
import { useRuntimeWorkspace } from "@/composables/useRuntimeWorkspace";
import { listJdks } from "@/api/jdk";
import { detectSpringBoot } from "@/api/springBoot";
import type { JdkInstallation } from "@/types/jdk";
import type { MavenProjectNode, RuntimeScope } from "@/types/maven";
import type { RuntimeApplicationConfig } from "@/types/runtime";
import { errMsg } from "@/utils/error";

const route = useRoute();
const router = useRouter();
// R-14 修复：向导此前未初始化 workspace（其他视图均走 useRuntimeWorkspace），
// 直接进入向导时 workspaceId 为空 → 项目列表恒为 no data。
const { workspaceStore, store, selectedWorkspaceId, ensureWorkspace, selectWorkspace } =
  useRuntimeWorkspace();

const isEdit = computed(() => !!route.query.edit);
const saving = ref(false);
const detecting = ref(false);
const resolving = ref(false);
const formRef = ref<FormInstance>();

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
/** R-14 §75：Pre/Post Build Script（首次执行必须确认）。 */
const preBuildScriptText = ref("");
const postBuildScriptText = ref("");

interface EnvRow {
  key: string;
  value: string;
}
const envRows = ref<EnvRow[]>([]);

const rules: FormRules = {
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
  };
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
    ElMessage.warning("请先选择 Maven 项目");
    return;
  }
  if (!store.workspaceId) {
    ElMessage.warning("未选择 workspace");
    return;
  }
  const ws = workspaceStore.workspaces.find((w) => w.id === store.workspaceId);
  if (!ws) return;
  detecting.value = true;
  try {
    const result = await detectSpringBoot(ws.path);
    // 匹配当前项目（path 后缀 / artifactId 两种口径都试）。
    const project = result.projects.find(
      (p) =>
        form.project === p.projectPath ||
        form.project.endsWith(p.projectPath) ||
        form.project.endsWith(p.module),
    );
    const candidate = project?.defaultMainClass || project?.candidates?.[0]?.className;
    if (candidate) {
      form.mainClass = candidate;
      ElMessage.success(`已预填 Main Class：${candidate}`);
    } else {
      ElMessage.info("该项目未检测到 Spring Boot Main Class，可手动填写");
    }
  } catch (e) {
    ElMessage.error("检测失败：" + errMsg(e));
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
    ElMessage.warning("未选择 workspace");
    return;
  }
  saving.value = true;
  try {
    await store.saveConfig(toConfig());
    ElMessage.success(isEdit.value ? "配置已保存" : "应用已创建");
    router.push({ name: "runtime-dashboard" });
  } catch (e) {
    ElMessage.error("保存失败：" + errMsg(e));
  } finally {
    saving.value = false;
  }
}

async function onResolve() {
  if (!store.workspaceId) return;
  resolving.value = true;
  try {
    await store.resolveDependencies();
    ElMessage.success("依赖解析任务已提交，完成后项目列表自动刷新");
  } catch (e) {
    ElMessage.error("解析失败：" + errMsg(e));
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
      ElMessage.error("加载配置失败：" + errMsg(e));
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
  padding: 12px 16px;
  gap: 12px;
  overflow-y: auto;
}
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
}
.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
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
  color: var(--el-text-color-secondary);
  margin-top: 4px;
  width: 100%;
}
.field-hint.warn-hint {
  color: var(--el-color-warning);
}
.script-field {
  width: 100%;
}
.project-field {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-start;
}
.projects-empty-alert {
  width: 100%;
  max-width: 560px;
}
.main-class-row {
  display: flex;
  gap: 8px;
  width: 100%;
}
.env-editor {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 8px;
  align-items: flex-start;
}
.muted {
  color: var(--el-text-color-secondary);
}
</style>
