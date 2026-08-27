<template>
  <div class="workspace-manage">
    <!-- Top toolbar -->
    <div class="toolbar">
      <div class="toolbar-left">
        <span class="page-title">工作区管理</span>
      </div>
      <div class="toolbar-right">
        <n-button type="primary" @click="showAdd = true">
          <template #icon><n-icon><AddOutline /></n-icon></template>
          添加工作区
        </n-button>
      </div>
    </div>

    <!-- Workspace cards -->
    <n-spin :show="workspaceStore.loading">
      <div v-if="workspaceStore.workspaces.length > 0" class="card-grid">
        <div v-for="ws in workspaceStore.workspaces" :key="ws.id" class="ws-card">
          <div class="ws-card-header">
            <span class="ws-name" :title="ws.name">{{ ws.name }}</span>
            <n-tag
              v-if="workspaceStore.currentWorkspace?.id === ws.id"
              size="small"
              type="success"
              :bordered="false"
            >
              当前
            </n-tag>
          </div>
          <div class="ws-path" :title="ws.path">
            <n-icon class="ws-path-icon"><FolderOpenOutline /></n-icon>
            <span class="ws-path-text">{{ ws.path }}</span>
          </div>
          <div class="ws-meta">
            <n-tag size="small" :bordered="false">扫描深度 {{ ws.scanDepth }}</n-tag>
          </div>
          <div class="ws-actions">
            <n-button size="small" @click="enterWorkspace(ws)">
              进入
            </n-button>
            <n-button size="small" @click="openEdit(ws)">
              <template #icon><n-icon><CreateOutline /></n-icon></template>
              编辑
            </n-button>
            <n-button size="small" quaternary type="error" @click="confirmRemove(ws)">
              <template #icon><n-icon><TrashOutline /></n-icon></template>
              删除
            </n-button>
          </div>
        </div>
      </div>
      <div v-else-if="!workspaceStore.loading" class="empty-state">
        <n-empty description="还没有工作区">
          <n-button type="primary" @click="showAdd = true">添加工作区</n-button>
        </n-empty>
      </div>
    </n-spin>

    <!-- Add workspace（复用现有组件） -->
    <WorkspaceManager v-model="showAdd" @added="onAdded" />

    <!-- Edit dialog：路径不可改（改动路径等同于新建工作区） -->
    <n-modal v-model:show="editDialog.show" preset="card" title="编辑工作区" style="width: 460px">
      <n-form :model="editDialog" label-width="100px">
        <n-form-item label="工作区名称" required>
          <n-input v-model:value="editDialog.name" placeholder="工作区名称" />
        </n-form-item>
        <n-form-item label="目录路径">
          <n-input :value="editDialog.path" disabled />
        </n-form-item>
        <n-form-item label="扫描深度">
          <n-input-number v-model:value="editDialog.scanDepth" :min="1" :max="20" :step="1" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="editDialog.show = false">取消</n-button>
        <n-button
          type="primary"
          :loading="editDialog.saving"
          :disabled="!editDialog.name.trim()"
          @click="handleEdit"
        >
          保存
        </n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { onMounted, reactive, ref } from "vue";
import { useRouter } from "vue-router";
import { useDialog, useMessage } from "naive-ui";
import {
  AddOutline,
  CreateOutline,
  FolderOpenOutline,
  TrashOutline,
} from "@vicons/ionicons5";
import WorkspaceManager from "@/components/common/WorkspaceManager.vue";
import { useWorkspaceStore } from "@/stores/workspace";
import { errMsg } from "@/utils/error";
import type { Workspace } from "@/types/workspace";

const router = useRouter();
const message = useMessage();
const dialog = useDialog();
const workspaceStore = useWorkspaceStore();

const showAdd = ref(false);

const editDialog = reactive({
  show: false,
  saving: false,
  id: 0,
  name: "",
  path: "",
  scanDepth: 5,
});

onMounted(() => {
  workspaceStore.loadWorkspaces();
});

function onAdded() {
  workspaceStore.loadWorkspaces();
}

/** 设为当前工作区并进入首页。 */
function enterWorkspace(ws: Workspace) {
  workspaceStore.selectWorkspace(ws);
  router.push({ name: "dashboard" });
}

function openEdit(ws: Workspace) {
  editDialog.id = ws.id;
  editDialog.name = ws.name;
  editDialog.path = ws.path;
  editDialog.scanDepth = ws.scanDepth;
  editDialog.show = true;
}

async function handleEdit() {
  editDialog.saving = true;
  try {
    await workspaceStore.updateWorkspace(editDialog.id, {
      name: editDialog.name.trim(),
      scanDepth: editDialog.scanDepth,
    });
    message.success("已保存");
    editDialog.show = false;
  } catch (e) {
    message.error("保存失败: " + errMsg(e));
  } finally {
    editDialog.saving = false;
  }
}

function confirmRemove(ws: Workspace) {
  dialog.warning({
    title: "删除工作区",
    content: `确定删除工作区「${ws.name}」吗？只会移除配置，不会删除磁盘上的任何文件。`,
    positiveText: "删除",
    negativeText: "取消",
    onPositiveClick: async () => {
      try {
        await workspaceStore.removeWorkspace(ws.id);
        message.success("已删除");
      } catch (e) {
        message.error("删除失败: " + errMsg(e));
      }
    },
  });
}
</script>

<style scoped>
.workspace-manage {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding: var(--gw-space-3) var(--gw-space-4);
  gap: var(--gw-space-3);
}

.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.toolbar-left {
  display: flex;
  gap: var(--gw-space-2);
  align-items: center;
}

.page-title {
  font-size: 15px;
  font-weight: 600;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: var(--gw-space-3);
  overflow-y: auto;
  padding-bottom: 8px;
}

.ws-card {
  border: 1px solid var(--gw-border);
  border-radius: 8px;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  transition: box-shadow 0.15s;
}

.ws-card:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}

.ws-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-2);
}

.ws-name {
  font-size: 15px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ws-path {
  display: flex;
  align-items: center;
  gap: 6px;
  color: #606266;
  font-size: 12px;
  min-width: 0;
}

.ws-path-icon {
  flex-shrink: 0;
}

.ws-path-text {
  font-family: monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.ws-meta {
  display: flex;
  gap: 6px;
}

.ws-actions {
  display: flex;
  gap: 6px;
  margin-top: auto;
}

.empty-state {
  padding: 60px 0;
}
</style>
