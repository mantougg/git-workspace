<template>
  <div class="group-tree">
    <n-tree
      :data="naiveTreeData"
      key-field="id"
      label-field="name"
      children-field="children"
      default-expand-all
      :selectable="true"
      :render-label="renderLabel"
      @update:selected-keys="onNodeSelect"
    />

    <div class="tree-footer">
      <n-button size="small" text @click="showCreateDialog = true">
        <template #icon><n-icon><AddOutline /></n-icon></template>
        添加分组
      </n-button>
    </div>

    <!-- Create group dialog -->
    <n-modal :show="showCreateDialog" preset="card" title="添加分组" style="width: 400px" @update:show="(v: boolean) => showCreateDialog = v">
      <n-form :model="newGroup" label-width="80px">
        <n-form-item label="分组名称">
          <n-input v-model:value="newGroup.name" placeholder="请输入分组名称" />
        </n-form-item>
      </n-form>
      <template #footer>
        <n-button @click="showCreateDialog = false">取消</n-button>
        <n-button type="primary" @click="handleCreate">创建</n-button>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch, h } from "vue";
import { useMessage } from "naive-ui";
import { AddOutline } from "@vicons/ionicons5";
import type { TreeOption } from "naive-ui";
import * as groupApi from "@/api/group";
import type { RepoGroup, CreateGroupRequest } from "@/types/group";
import { errMsg } from "@/utils/error";

const message = useMessage();

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

const naiveTreeData = computed(() => treeData.value as unknown as TreeOption[]);

function renderLabel({ option }: { option: TreeOption }) {
  const node = option as unknown as TreeNode;
  return h("span", { class: "tree-node" }, [
    h("span", { class: "node-label" }, node.name),
    node.repoCount
      ? h("span", { class: "node-count" }, ` (${node.repoCount})`)
      : null,
  ]);
}

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

function onNodeSelect(keys: number[]) {
  const id = keys[0];
  if (id === undefined) return;
  if (id === -1) {
    emit("select-group", null);
  } else {
    emit("select-group", id);
  }
}

async function handleCreate() {
  if (!props.workspaceId) return;
  if (!newGroup.value.name.trim()) {
    message.warning("请输入分组名称");
    return;
  }

  try {
    const req: CreateGroupRequest = {
      workspaceId: props.workspaceId,
      name: newGroup.value.name,
      parentId: null,
    };
    await groupApi.createGroup(req);
    message.success("分组创建成功");
    newGroup.value.name = "";
    showCreateDialog.value = false;
    await loadGroups();
  } catch (e) {
    message.error("创建分组失败: " + errMsg(e));
  }
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
