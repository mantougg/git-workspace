<template>
  <div class="group-tree">
    <el-tree
      :data="treeData"
      node-key="id"
      :props="treeProps"
      default-expand-all
      @node-click="onNodeClick"
      :allow-drop="allowDrop"
      :allow-drag="allowDrag"
      draggable
    >
      <template #default="{ node, data }">
        <span class="tree-node">
          <span class="node-label">{{ node.label }}</span>
          <span v-if="data.repoCount" class="node-count">
            ({{ data.repoCount }})
          </span>
        </span>
      </template>
    </el-tree>

    <div class="tree-footer">
      <el-button size="small" text @click="showCreateDialog = true">
        <el-icon><Plus /></el-icon>
        添加分组
      </el-button>
    </div>

    <!-- Create group dialog -->
    <el-dialog v-model="showCreateDialog" title="添加分组" width="400px">
      <el-form :model="newGroup" label-width="80px">
        <el-form-item label="分组名称">
          <el-input v-model="newGroup.name" placeholder="请输入分组名称" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="showCreateDialog = false">取消</el-button>
        <el-button type="primary" @click="handleCreate">创建</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Plus } from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import * as groupApi from "@/api/group";
import type { RepoGroup, CreateGroupRequest } from "@/types/group";
import { errMsg } from "@/utils/error";

const props = defineProps<{
  workspaceId: number | null;
  repos: { groupId: number | null }[];
}>();

const emit = defineEmits<{
  (e: "select-group", groupId: number | null): void;
}>();

const groups = ref<RepoGroup[]>([]);
const showCreateDialog = ref(false);
const newGroup = ref({ name: "" });

const treeProps = {
  label: "name",
  children: "children",
};

interface TreeNode {
  id: number;
  name: string;
  parentId: number | null;
  children: TreeNode[];
  repoCount: number;
}

const treeData = computed<TreeNode[]>(() => {
  const buildTree = (
    parentId: number | null,
    groupList: RepoGroup[],
  ): TreeNode[] => {
    return groupList
      .filter((g) => g.parentId === parentId)
      .map((g) => {
        const children = buildTree(g.id, groupList);
        const repoCount = props.repos.filter(
          (r) => r.groupId === g.id,
        ).length;
        return {
          id: g.id,
          name: g.name,
          parentId: g.parentId,
          children,
          repoCount,
        };
      });
  };

  const tree = buildTree(null, groups.value);

  // Add "All Repositories" node
  return [
    {
      id: -1,
      name: "全部仓库",
      parentId: null,
      children: tree,
      repoCount: props.repos.length,
    },
  ];
});

watch(
  () => props.workspaceId,
  async (newId) => {
    if (newId) {
      await loadGroups();
    }
  },
  { immediate: true },
);

async function loadGroups() {
  if (!props.workspaceId) return;
  try {
    groups.value = await groupApi.listGroups(props.workspaceId);
  } catch (e) {
    console.error("Failed to load groups:", e);
  }
}

function onNodeClick(data: TreeNode) {
  if (data.id === -1) {
    emit("select-group", null);
  } else {
    emit("select-group", data.id);
  }
}

async function handleCreate() {
  if (!props.workspaceId) return;
  if (!newGroup.value.name.trim()) {
    ElMessage.warning("请输入分组名称");
    return;
  }

  try {
    const req: CreateGroupRequest = {
      workspaceId: props.workspaceId,
      name: newGroup.value.name,
      parentId: null,
    };
    await groupApi.createGroup(req);
    ElMessage.success("分组创建成功");
    newGroup.value.name = "";
    showCreateDialog.value = false;
    await loadGroups();
  } catch (e) {
    ElMessage.error("创建分组失败: " + errMsg(e));
  }
}

function allowDrop(): boolean {
  return true;
}

function allowDrag(): boolean {
  return true;
}
</script>

<style scoped>
.group-tree {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 4px;
}

.node-label {
  font-size: 13px;
}

.node-count {
  font-size: 12px;
  color: #909399;
}

.tree-footer {
  padding: 8px;
  border-top: 1px solid #ebeef5;
}
</style>
