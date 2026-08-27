<template>
  <div class="change-tree">
    <n-tree
      ref="treeRef"
      :data="naiveTreeData"
      key-field="key"
      :selectable="false"
      checkable
      :expanded-keys="expandedKeysState"
      :cascade="true"
      :render-label="renderLabel"
      :render-prefix="renderPrefix"
      :render-suffix="renderSuffix"
      @update:checked-keys="onCheck"
      @update:expanded-keys="onExpandedChange"
      class="tree"
    />

    <div v-if="changes.length === 0" class="empty-tree">
      <n-empty description="暂无仓库数据" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, watch, h } from "vue";
import { DocumentTextOutline, FolderOutline, FolderOpenOutline } from "@vicons/ionicons5";
import { NIcon, NTag } from "naive-ui";
import type { TreeOption } from "naive-ui";
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

const naiveTreeData = computed(() => treeData.value as unknown as TreeOption[]);

/** Default-expand the top level (top dirs + repos at the workspace root). */
const expandedKeys = computed(() => treeData.value.map((n) => n.key));

/** F-09a/g：受控展开状态（双击整行展开/收起、展开全部/收起全部）。 */
const expandedKeysState = ref<string[]>([]);

/** F-09d/e：最近一次勾选 keys（naive-ui Tree 只有 getCheckedData，没有
 *  getCheckedKeys——旧代码调用不存在的方法导致勾选状态永远同步不上来）。 */
const lastCheckedKeys = ref<string[]>([]);

watch(
  expandedKeys,
  (keys) => {
    // 数据变化时恢复「默认展开顶层」行为。
    expandedKeysState.value = [...keys];
  },
  { immediate: true },
);

watch(
  () => props.changes,
  () => {
    emitSelection();
  },
  { deep: true },
);

function onExpandedChange(keys: Array<string | number>) {
  expandedKeysState.value = keys.map(String);
}

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

function onCheck(checkedKeys: Array<string | number>) {
  lastCheckedKeys.value = checkedKeys.map(String);
  emitSelection();
}

/** Double-click on any part of a row: expand/collapse non-leaf, diff for files. */
function onNodeDblClick(data: ChangeNode) {
  if (data.type === "file") {
    emit("file-dblclick", data);
    return;
  }
  // F-09a：非叶子节点整行双击 → 展开/收起子级（不必点折叠箭头）。
  const keys = expandedKeysState.value;
  const index = keys.indexOf(data.key);
  expandedKeysState.value =
    index >= 0 ? keys.filter((key) => key !== data.key) : [...keys, data.key];
}

function renderLabel({ option }: { option: TreeOption }) {
  const data = option as unknown as ChangeNode;
  const parts = [];

  if (data.type === "repo" || (data.type === "dir" && data.repoPath && !data.relPath)) {
    parts.push(h("span", { class: "node-label repo-name" }, data.label));
    if (data.branch) {
      parts.push(
        h(NTag, { size: "small", type: "info", bordered: false, class: "branch-tag" }, () => data.branch),
      );
    }
    if (data.ahead && data.ahead > 0 || data.behind && data.behind > 0) {
      const infoParts = [];
      if (data.ahead && data.ahead > 0) infoParts.push(h("span", { class: "ahead" }, `↑${data.ahead}`));
      if (data.behind && data.behind > 0) infoParts.push(h("span", { class: "behind" }, `↓${data.behind}`));
      parts.push(h("span", { class: "remote-info" }, infoParts));
    }
    // F-09c：变更状态用 NTag 语义化颜色展示。
    if (data.changeCount && data.changeCount > 0) {
      parts.push(
        h(NTag, { size: "small", type: "warning", bordered: false }, () => `${data.changeCount} 处变更`),
      );
      parts.push(
        h(NTag, { size: "small", type: "warning", bordered: false }, () => "已修改"),
      );
    } else {
      parts.push(
        h(NTag, { size: "small", type: "success", bordered: false }, () => "未变更"),
      );
    }
  } else if (data.type === "dir") {
    parts.push(h("span", { class: "node-label" }, `${data.label}/`));
  } else {
    parts.push(h("span", { class: "node-label" }, data.label));
    parts.push(
      h(
        NTag,
        { size: "small", type: statusTagType(data.status || ""), bordered: false },
        () => statusText(data.status || ""),
      ),
    );
  }

  return h(
    "div",
    {
      class: `tree-node type-${data.type}`,
      onDblclick: () => onNodeDblClick(data),
    },
    parts,
  );
}

function renderPrefix({ option }: { option: TreeOption }) {
  const data = option as unknown as ChangeNode;
  if (data.type === "dir" && !data.repoPath) {
    return h(NIcon, { size: 16, class: "node-icon" }, () => h(FolderOutline));
  } else if (data.type === "repo" || (data.type === "dir" && data.repoPath && !data.relPath)) {
    return h(NIcon, { size: 16, class: "node-icon repo-icon" }, () => h(FolderOpenOutline));
  } else if (data.type === "dir") {
    return h(NIcon, { size: 16, class: "node-icon" }, () => h(FolderOpenOutline));
  } else {
    return h(NIcon, { size: 16, class: `node-icon status-${data.status}` }, () => h(DocumentTextOutline));
  }
}

function renderSuffix() {
  return null;
}

function emitSelection() {
  const checkedKeys = lastCheckedKeys.value;
  const allNodes = flattenTree(treeData.value);
  const checkedNodes = allNodes.filter((n) => checkedKeys.includes(n.key));

  const repoPaths = new Set<string>();
  const filesByRepo = new Map<string, string[]>();

  for (const node of checkedNodes) {
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

function flattenTree(nodes: ChangeNode[]): ChangeNode[] {
  const result: ChangeNode[] = [];
  for (const n of nodes) {
    result.push(n);
    if (n.children) result.push(...flattenTree(n.children));
  }
  return result;
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

/** F-09c：文件状态 → NTag 语义化颜色。 */
function statusTagType(
  status: string,
): "default" | "info" | "success" | "warning" | "error" {
  const map: Record<string, "default" | "info" | "success" | "warning" | "error"> = {
    untracked: "default",
    modified: "warning",
    deleted: "error",
    added: "success",
    renamed: "info",
    typechange: "warning",
  };
  return map[status] ?? "default";
}

/** 收集所有有子级的节点 key（用于「展开全部」）。 */
function collectParentKeys(nodes: ChangeNode[]): string[] {
  const keys: string[] = [];
  for (const n of nodes) {
    if (n.children && n.children.length > 0) {
      keys.push(n.key);
      keys.push(...collectParentKeys(n.children));
    }
  }
  return keys;
}

defineExpose({
  refresh: () => emitSelection(),
  // F-09g：原来是空实现，按钮点击无反应。
  expandAll: () => {
    expandedKeysState.value = collectParentKeys(treeData.value);
  },
  collapseAll: () => {
    expandedKeysState.value = [];
  },
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
