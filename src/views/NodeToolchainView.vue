<template>
  <div class="node-toolchain-view">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <n-button :loading="pruning" @click="onPrune">
          <template #icon><n-icon><TrashOutline /></n-icon></template>
          清理失效条目
        </n-button>
      </div>
      <div class="toolbar-right">
        <n-button type="success" dashed @click="openAddModal">
          <template #icon><n-icon><AddOutline /></n-icon></template>
          注册可执行文件
        </n-button>
      </div>
    </div>

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
      <span class="summary-hint">
        注册条目在工具链决策链中优先于 PATH 自动检测；执行器来源会在 Node 项目启动详情中展示
      </span>
    </div>

    <n-spin :show="loading">
      <n-data-table
        :columns="columns"
        :data="executables"
        :row-key="(row: NodeExecutable) => row.id ?? ''"
        empty-text="暂无注册条目；Node/npm/pnpm/yarn 仍可通过 PATH 自动检测"
      />
    </n-spin>

    <!-- Add modal -->
    <n-modal
      v-model:show="addVisible"
      preset="card"
      title="注册 Node 可执行文件"
      class="add-modal"
    >
      <n-form label-placement="left" :label-width="90">
        <n-form-item label="类型">
          <n-radio-group v-model:value="addForm.kind">
            <n-radio value="node">node</n-radio>
            <n-radio value="packageManager">包管理器</n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item v-if="addForm.kind === 'packageManager'" label="包管理器">
          <n-radio-group v-model:value="addForm.packageManager">
            <n-radio value="npm">npm</n-radio>
            <n-radio value="pnpm">pnpm</n-radio>
            <n-radio value="yarn">yarn</n-radio>
          </n-radio-group>
        </n-form-item>
        <n-form-item label="可执行文件">
          <n-input-group>
            <n-input
              v-model:value="addForm.executablePath"
              placeholder="node / npm / pnpm / yarn 的可执行文件完整路径"
              class="mono"
            />
            <n-button @click="browseExecutable">浏览</n-button>
          </n-input-group>
        </n-form-item>
        <n-alert :type="addProbeState.type" v-if="addProbeState" :show-icon="true">
          {{ addProbeState.text }}
        </n-alert>
      </n-form>
      <template #footer>
        <n-space justify="end">
          <n-button @click="addVisible = false">取消</n-button>
          <n-button type="primary" :loading="adding" @click="onAdd">注册</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, h, onMounted, ref } from "vue";
import {
  NAlert,
  NButton,
  NDataTable,
  NForm,
  NFormItem,
  NIcon,
  NInput,
  NInputGroup,
  NModal,
  NRadio,
  NRadioGroup,
  NSpin,
  NTag,
  useMessage,
} from "naive-ui";
import { AddOutline, TrashOutline } from "@vicons/ionicons5";
import { open } from "@tauri-apps/plugin-dialog";
import {
  nodeAddExecutable,
  nodeListExecutables,
  nodePruneExecutables,
  nodeRemoveExecutable,
  nodeValidateExecutable,
} from "@/api/node";
import type { NodeExecutable, NodePackageManager } from "@/types/node";
import { errMsg } from "@/utils/error";

const message = useMessage();

const executables = ref<NodeExecutable[]>([]);
const loading = ref(false);
const pruning = ref(false);
const validatingId = ref<number | null>(null);
const adding = ref(false);

const validCount = computed(() => executables.value.filter((e) => e.isValid).length);
const invalidCount = computed(() => executables.value.filter((e) => !e.isValid).length);

const addVisible = ref(false);
const addForm = ref({
  kind: "node" as "node" | "packageManager",
  packageManager: "npm" as NodePackageManager,
  executablePath: "",
});
const addProbeState = ref<{ type: "info" | "success" | "error"; text: string } | null>(null);

const columns = [
  {
    title: "状态",
    key: "status",
    width: 80,
    render(row: NodeExecutable) {
      return h(
        NTag,
        { type: row.isValid ? "success" : "error", size: "small", bordered: false },
        { default: () => (row.isValid ? "有效" : "失效") },
      );
    },
  },
  {
    title: "类型",
    key: "kind",
    width: 110,
    render(row: NodeExecutable) {
      return h(
        NTag,
        { size: "small", type: "info", bordered: true },
        { default: () => (row.kind === "node" ? "node" : "包管理器") },
      );
    },
  },
  {
    title: "包管理器",
    key: "packageManager",
    width: 100,
    render(row: NodeExecutable) {
      return row.packageManager
        ? h("span", null, row.packageManager)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "版本",
    key: "version",
    width: 120,
    render(row: NodeExecutable) {
      return row.version
        ? h("span", { class: "mono" }, row.version)
        : h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "可执行文件路径",
    key: "executablePath",
    minWidth: 280,
    ellipsis: { tooltip: true },
    render(row: NodeExecutable) {
      return h("span", { class: "mono" }, row.executablePath);
    },
  },
  {
    title: "最近校验",
    key: "lastChecked",
    width: 170,
    render(row: NodeExecutable) {
      if (row.lastChecked) {
        return h("span", { class: "muted" }, formatTime(row.lastChecked));
      }
      return h("span", { class: "muted" }, "—");
    },
  },
  {
    title: "操作",
    key: "actions",
    width: 160,
    fixed: "right" as const,
    render(row: NodeExecutable) {
      return h("div", { style: "display:flex;gap:8px" }, [
        h(
          NButton,
          {
            size: "small",
            loading: validatingId.value === row.id,
            onClick: () => onValidate(row),
          },
          { default: () => "复检" },
        ),
        h(
          NButton,
          { size: "small", type: "error", dashed: true, onClick: () => onRemove(row) },
          { default: () => "删除" },
        ),
      ]);
    },
  },
];

async function reload() {
  loading.value = true;
  try {
    executables.value = await nodeListExecutables();
  } catch (e) {
    message.error("加载注册列表失败：" + errMsg(e));
  } finally {
    loading.value = false;
  }
}

function openAddModal() {
  addForm.value = { kind: "node", packageManager: "npm", executablePath: "" };
  addProbeState.value = null;
  addVisible.value = true;
}

async function browseExecutable() {
  try {
    const selected = await open({
      directory: false,
      multiple: false,
      title: "选择 node / npm / pnpm / yarn 可执行文件",
    });
    if (typeof selected === "string" && selected) {
      addForm.value.executablePath = selected;
      addProbeState.value = null;
    }
  } catch (e) {
    message.error("打开文件选择器失败：" + errMsg(e));
  }
}

async function onAdd() {
  const path = addForm.value.executablePath.trim();
  if (!path) {
    addProbeState.value = { type: "error", text: "请填写或选择可执行文件路径" };
    return;
  }
  adding.value = true;
  try {
    const added = await nodeAddExecutable({
      kind: addForm.value.kind,
      packageManager: addForm.value.kind === "packageManager" ? addForm.value.packageManager : null,
      executablePath: path,
    });
    message.success(
      `已注册 ${added.executablePath}${added.version ? `（${added.version}）` : ""}`,
    );
    addVisible.value = false;
    await reload();
  } catch (e) {
    // 版本探测失败等可行动错误直接透出后端提示。
    addProbeState.value = { type: "error", text: errMsg(e) };
  } finally {
    adding.value = false;
  }
}

async function onValidate(row: NodeExecutable) {
  if (row.id == null) return;
  validatingId.value = row.id;
  try {
    const updated = await nodeValidateExecutable(row.id);
    const idx = executables.value.findIndex((e) => e.id === row.id);
    if (idx >= 0) executables.value[idx] = updated;
    message.success(updated.isValid ? "复检通过" : "复检失败，已标记失效");
  } catch (e) {
    message.error("复检失败：" + errMsg(e));
  } finally {
    validatingId.value = null;
  }
}

async function onRemove(row: NodeExecutable) {
  if (row.id == null) return;
  try {
    await nodeRemoveExecutable(row.id);
    executables.value = executables.value.filter((e) => e.id !== row.id);
    message.success("已删除注册条目");
  } catch (e) {
    message.error("删除失败：" + errMsg(e));
  }
}

async function onPrune() {
  pruning.value = true;
  try {
    const n = await nodePruneExecutables();
    if (n > 0) {
      message.success(`已移除 ${n} 个路径已失效的条目`);
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

function formatTime(iso: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

onMounted(reload);
</script>

<style scoped>
.node-toolchain-view {
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
.summary {
  display: flex;
  gap: 20px;
  align-items: center;
  margin-bottom: 12px;
  font-size: 14px;
  color: var(--gw-text);
  flex-wrap: wrap;
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
.muted {
  color: var(--gw-text-dim);
}
.mono {
  font-family: var(--gw-font-mono);
  font-size: 12px;
}
.add-modal {
  width: 560px;
}
</style>
