<template>
  <div class="change-tree">
    <el-tree
      ref="treeRef"
      :data="treeData"
      node-key="key"
      show-checkbox
      :default-expanded-keys="expandedKeys"
      :expand-on-click-node="false"
      :props="{ label: 'label', children: 'children' }"
      @check="onCheck"
      class="tree"
    >
      <template #default="{ data }">
        <!-- @click.stop: block el-tree's node-click handling so selection/checking
             only happens via the checkbox, never via label clicks or double-clicks.
             @dblclick.stop: double-click anywhere on the row to expand/diff. -->
        <div
          class="tree-node"
          :class="`type-${data.type}`"
          @click.stop
          @dblclick.stop="onNodeDblClick(data)"
        >
          <!-- Workspace directory node (not a repo itself) -->
          <template v-if="data.type === 'dir' && !data.repoPath">
            <el-icon class="node-icon"><Folder /></el-icon>
            <span class="node-label">{{ data.label }}</span>
          </template>

          <!-- Repo node: top-level repo OR a directory that is itself a git repo -->
          <template v-else-if="data.type === 'repo' || (data.type === 'dir' && data.repoPath && !data.relPath)">
            <el-icon class="node-icon repo-icon"><FolderOpened /></el-icon>
            <span class="node-label repo-name">{{ data.label }}</span>
            <el-tag
              v-if="data.branch"
              size="small"
              effect="plain"
              class="branch-tag"
            >
              {{ data.branch }}
            </el-tag>
            <span
              v-if="data.ahead > 0 || data.behind > 0"
              class="remote-info"
            >
              <span v-if="data.ahead > 0" class="ahead">↑{{ data.ahead }}</span>
              <span v-if="data.behind > 0" class="behind">↓{{ data.behind }}</span>
            </span>
            <span
              v-if="data.changeCount > 0"
              class="change-badge"
            >
              {{ data.changeCount }} 处变更
            </span>
            <span v-else class="clean-text">无变更</span>
          </template>

          <!-- File-tree dir node (inside a repo) -->
          <template v-else-if="data.type === 'dir'">
            <el-icon class="node-icon"><FolderOpened /></el-icon>
            <span class="node-label">{{ data.label }}/</span>
          </template>

          <!-- File node -->
          <template v-else>
            <el-icon class="node-icon" :class="`status-${data.status}`">
              <Document />
            </el-icon>
            <span class="node-label">{{ data.label }}</span>
            <span class="file-status" :class="`status-${data.status}`">
              {{ statusText(data.status) }}
            </span>
          </template>
        </div>
      </template>
    </el-tree>

    <div v-if="changes.length === 0" class="empty-tree">
      <el-empty description="暂无仓库数据" :image-size="60" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Document, Folder, FolderOpened } from "@element-plus/icons-vue";
import type { RepoChanges } from "@/types/changes";

export interface ChangeNode {
  key: string;
  label: string;
  type: "repo" | "dir" | "file";
  /** Absolute repo path — present on repo nodes and on nodes inside a repo. */
  repoPath?: string;
  /** Path relative to the repo root (file-tree nodes only). */
  relPath?: string;
  status?: string;
  branch?: string;
  isDetached?: boolean;
  ahead?: number;
  behind?: number;
  changeCount?: number;
  children?: ChangeNode[];
}

export interface TreeSelection {
  /** Checked repository paths (deduplicated, includes repos of checked files). */
  repoPaths: string[];
  /** repoPath -> checked file paths relative to that repo. */
  filesByRepo: Map<string, string[]>;
}

const props = defineProps<{
  changes: RepoChanges[];
}>();

const emit = defineEmits<{
  (e: "selection-change", selection: TreeSelection): void;
  (e: "file-dblclick", node: ChangeNode): void;
}>();

const treeRef = ref();

const treeData = computed<ChangeNode[]>(() => buildTree(props.changes));

/** Default-expand the top level (top dirs + repos at the workspace root). */
const expandedKeys = computed(() => treeData.value.map((n) => n.key));

watch(
  () => props.changes,
  () => {
    emitSelection();
  },
  { deep: true },
);

function normalizeRel(p: string): string {
  return p.replace(/[\\/]+/g, "/").replace(/^\/+|\/+$/g, "");
}

/**
 * Build workspace → dir → repo → (dir → file) tree.
 *
 * A directory that is itself a git repository is merged with the repo node:
 * the dir node carries the repo props (branch / ahead / behind / changeCount)
 * and its children hold both the repo's changed files and its sub-directories.
 */
function buildTree(changes: RepoChanges[]): ChangeNode[] {
  const roots: ChangeNode[] = [];
  const dirMap = new Map<string, ChangeNode>();
  const repoByRel = new Map<string, RepoChanges>();

  for (const r of changes) {
    repoByRel.set(normalizeRel(r.relativePath), r);
  }

  for (const repo of changes) {
    const parts = normalizeRel(repo.relativePath).split("/").filter(Boolean);
    let parentChildren = roots;
    let curRel = "";
    for (const part of parts) {
      curRel = curRel ? `${curRel}/${part}` : part;
      let node = dirMap.get(curRel);
      if (!node) {
        const isRepoDir = repoByRel.has(curRel);
        // Reuse an existing same-named dir node (e.g. created by a repo's
        // file tree) instead of creating a sibling duplicate.
        const existing = parentChildren.find(
          (n) => n.type === "dir" && n.label === part,
        );
        if (existing) {
          node = existing;
          if (isRepoDir) attachRepoProps(node, repoByRel.get(curRel)!);
        } else {
          node = {
            key: isRepoDir ? `repo:${curRel}` : `dir:${curRel}`,
            label: part,
            type: "dir",
            children: [],
          };
          if (isRepoDir) attachRepoProps(node, repoByRel.get(curRel)!);
          parentChildren.push(node);
        }
        // Always register the node under its path so later lookups by the
        // full relative path (e.g. `dirMap.get(repo.relativePath)`) succeed
        // even when the node was reused from a repo file tree.
        dirMap.set(curRel, node);
      }
      parentChildren = node.children!;
    }

    // The last segment is this repo's own dir node — build its change-file tree.
    const repoDirNode = dirMap.get(normalizeRel(repo.relativePath))!;
    buildRepoFileTree(repoDirNode, repo);
  }

  sortTree(roots);
  return roots;
}

function attachRepoProps(node: ChangeNode, repo: RepoChanges) {
  node.repoPath = repo.repoPath;
  node.branch = repo.branch;
  node.isDetached = repo.isDetached;
  node.ahead = repo.ahead;
  node.behind = repo.behind;
  node.changeCount = repo.changes.length;
}

/** Build the dir/file change tree under a repo dir node. */
function buildRepoFileTree(repoDirNode: ChangeNode, repo: RepoChanges) {
  const dirMap = new Map<string, ChangeNode>();
  for (const f of repo.changes) {
    const parts = f.path.split("/");
    const dirPath = parts.slice(0, -1).join("/");

    // Untracked directory entries come as "dir/" — ensure the dir node exists
    // so it is visible and staging can pass the whole directory path.
    if (f.path.endsWith("/") && parts.length >= 2) {
      // "src/themeData/" -> dirName "themeData", parent path "src"
      const trimmed = f.path.replace(/\/+$/, "");
      const tparts = trimmed.split("/");
      const dirName = tparts[tparts.length - 1];
      const dirPath = tparts.slice(0, -1).join("/");
      const parentChildren = findOrCreateDirChain(repoDirNode, dirMap, dirPath);
      const existing = parentChildren.find(
        (n) => n.type === "dir" && n.label === dirName,
      );
      if (!existing) {
        parentChildren.push({
          key: `dir:${repo.repoPath}:${f.path}`,
          label: dirName,
          type: "dir",
          repoPath: repo.repoPath,
          relPath: f.path,
          status: f.status,
          children: [],
        });
      }
      continue;
    }

    const parentChildren = findOrCreateDirChain(repoDirNode, dirMap, dirPath);
    parentChildren.push({
      key: `file:${repo.repoPath}:${f.path}`,
      label: parts[parts.length - 1],
      type: "file",
      repoPath: repo.repoPath,
      relPath: f.path,
      status: f.status,
    });
  }
}

/**
 * Create (or reuse) the chain of dir nodes for a path, returning its children array.
 * Existing same-named dir nodes (e.g. workspace dirs holding sub-repos) are reused
 * so a directory never appears twice at the same level.
 */
function findOrCreateDirChain(
  repoDirNode: ChangeNode,
  dirMap: Map<string, ChangeNode>,
  dirPath: string,
): ChangeNode[] {
  if (!dirPath) return repoDirNode.children!;
  const parts = dirPath.split("/");
  let curPath = "";
  let parentChildren = repoDirNode.children!;
  for (const part of parts) {
    curPath = curPath ? `${curPath}/${part}` : part;
    let dirNode = dirMap.get(curPath);
    if (!dirNode) {
      const existing = parentChildren.find(
        (n) => n.type === "dir" && n.label === part,
      );
      if (existing) {
        dirNode = existing;
      } else {
        dirNode = {
          key: `dir:${repoDirNode.repoPath}:${curPath}`,
          label: part,
          type: "dir",
          repoPath: repoDirNode.repoPath,
          relPath: curPath,
          children: [],
        };
        parentChildren.push(dirNode);
      }
      dirMap.set(curPath, dirNode);
    }
    parentChildren = dirNode.children!;
  }
  return parentChildren;
}

/** Directories first, then repositories; alphabetically within each group. */
function sortTree(nodes: ChangeNode[]) {
  nodes.sort((a, b) => {
    const aDir = a.type === "dir" ? 0 : 1;
    const bDir = b.type === "dir" ? 0 : 1;
    if (aDir !== bDir) return aDir - bDir;
    return a.label.localeCompare(b.label);
  });
  for (const n of nodes) {
    if (n.children) sortTree(n.children);
  }
}

function onCheck() {
  emitSelection();
}

/** Double-click on any part of a row: expand/collapse non-leaf, diff for files. */
function onNodeDblClick(data: ChangeNode) {
  if (data.type === "file") {
    emit("file-dblclick", data);
  } else {
    toggleExpand(data);
  }
}

/** Toggle expansion of a folder/repo node. */
function toggleExpand(data: ChangeNode) {
  const node = treeRef.value?.getNode(data.key);
  if (node) {
    node.expanded = !node.expanded;
  }
}

function emitSelection() {
  if (!treeRef.value) return;
  const checked = treeRef.value.getCheckedNodes(false, false) as ChangeNode[];
  const repoPaths = new Set<string>();
  const filesByRepo = new Map<string, string[]>();

  for (const node of checked) {
    if (!node.repoPath) continue;
    repoPaths.add(node.repoPath);
    // Collect file paths, plus leaf dir nodes (untracked directories like "packages/").
    const isCollectible =
      node.type === "file" ||
      (node.type === "dir" && (!node.children || node.children.length === 0));
    if (isCollectible && node.relPath) {
      const arr = filesByRepo.get(node.repoPath) ?? [];
      arr.push(node.relPath);
      filesByRepo.set(node.repoPath, arr);
    }
  }

  emit("selection-change", {
    repoPaths: [...repoPaths],
    filesByRepo,
  });
}

function setAllExpanded(expanded: boolean) {
  const store = treeRef.value?.store;
  if (!store) return;
  const nodes = store._getAllNodes();
  for (const n of nodes) {
    n.expanded = expanded;
  }
}

function statusText(status: string): string {
  const map: Record<string, string> = {
    untracked: "未跟踪",
    modified: "已修改",
    deleted: "已删除",
    added: "已暂存",
    renamed: "已重命名",
    typechange: "类型变更",
  };
  return map[status] ?? status;
}

defineExpose({
  refresh: () => emitSelection(),
  expandAll: () => setAllExpanded(true),
  collapseAll: () => setAllExpanded(false),
});
</script>

<style scoped>
.change-tree {
  height: 100%;
  overflow-y: auto;
  padding: 4px 0;
}

.tree {
  background: transparent;
}

/* Make the slot fill the whole row so double-clicking anywhere on the
   row (not just the label text) triggers expand/diff. */
:deep(.el-tree-node__label) {
  flex: 1;
  min-width: 0;
}

.tree-node {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  padding-right: 8px;
  width: 100%;
  user-select: none;
  -webkit-user-select: none;
}

.node-icon {
  color: #909399;
  flex-shrink: 0;
}

.repo-icon {
  color: #409eff;
}

.node-label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.repo-name {
  font-weight: 600;
}

.branch-tag {
  flex-shrink: 0;
}

.remote-info {
  font-size: 12px;
  flex-shrink: 0;
}

.ahead {
  color: #67c23a;
  margin-right: 4px;
}

.behind {
  color: #e6a23c;
}

.change-badge {
  font-size: 12px;
  color: #f56c6c;
  background: #fef0f0;
  border-radius: 8px;
  padding: 0 6px;
  flex-shrink: 0;
}

.clean-text {
  font-size: 12px;
  color: #c0c4cc;
  flex-shrink: 0;
}

.file-status {
  font-size: 12px;
  flex-shrink: 0;
}

.status-untracked {
  color: #909399;
}

.status-modified {
  color: #e6a23c;
}

.status-deleted {
  color: #f56c6c;
}

.status-added {
  color: #67c23a;
}

.status-renamed {
  color: #a855f7;
}

.status-typechange {
  color: #ff7d00;
}

.empty-tree {
  padding: 40px 0;
}
</style>
