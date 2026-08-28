// R-20 §45 Runtime Dependency Graph 树数据组装（纯函数，无副作用）。
//
// 层次（§45）：应用 → 模块 → 库依赖。数据一次性从 R-02 依赖图查询结果
// （DependencyGraphView）+ Scope 闭包（RuntimeClosure）组装，前端只做展示
// 与交互（任务文档「架构/性能注意点」）。
//
// 大图性能：workspace 模块子树**按 projectId 记忆化**——被多个模块共享的
// 依赖（如 common）只构建一次、多处复用（DAG 展开 ≠ 树复制）；展平行 key
// 携带路径前缀保证唯一；配合折叠 + n-virtual-list 虚拟化，DOM 与视口内
// 行数成正比而与图规模无关。
//
// 着色语义（三态，颜色配文字/图标，不单独依赖色相）：
// - workspaceSource  源码（含相对路径）
// - localRepository  本地 Maven 仓库
// - remoteRepository 远程 Maven 仓库（本地缺失，构建时解析）

import type {
  DependencyEdge,
  MavenProjectNode,
  RuntimeClosure,
} from "@/types/maven";
import type { DependencyGraphView } from "@/types/runtime";

export type DependencyNodeSource =
  | "workspaceSource"
  | "localRepository"
  | "remoteRepository";

/** 依赖树节点。app = 闭包根应用；module = workspace 源码模块；library = 外部库。 */
export interface TreeNode {
  key: string;
  kind: "app" | "module" | "library";
  /** module / app 的 workspace projectId；library 为 null。 */
  projectId: number | null;
  /** library 的来源边；module / app 为 null。 */
  edge: DependencyEdge | null;
  label: string;
  /** `groupId:artifactId[:version]`。 */
  coordinates: string;
  version: string | null;
  /** module：POM 路径；library：解析到的本地路径（remote 为 null）。 */
  path: string | null;
  /** 节点来源（app/module 恒为 workspaceSource；library 为 local/remote）。 */
  source: DependencyNodeSource;
  children: TreeNode[];
}

function coords(c: { groupId: string; artifactId: string; version?: string | null }): string {
  return c.version ? `${c.groupId}:${c.artifactId}:${c.version}` : `${c.groupId}:${c.artifactId}`;
}

/** 组装依赖树。`closure` 决定展示哪些 workspace 模块（Scope 联动）。 */
export function buildDependencyTree(
  graph: DependencyGraphView,
  closure: RuntimeClosure,
): TreeNode {
  const projectById = new Map<number, MavenProjectNode>();
  for (const p of graph.projects) projectById.set(p.projectId, p);
  const closureIds = new Set(closure.projects.map((p) => p.projectId));

  const edgesByFrom = new Map<number, DependencyEdge[]>();
  for (const e of graph.dependencies) {
    const list = edgesByFrom.get(e.fromProjectId);
    if (list) list.push(e);
    else edgesByFrom.set(e.fromProjectId, [e]);
  }

  const rootId = projectById.has(closure.rootProjectId)
    ? closure.rootProjectId
    : closure.projects[0]?.projectId;
  if (rootId == null) {
    // 空闭包：返回占位 app 节点（UI 显示空态提示）。
    return {
      key: "app:empty",
      kind: "app",
      projectId: null,
      edge: null,
      label: "（闭包为空）",
      coordinates: "",
      version: null,
      path: null,
      source: "workspaceSource",
      children: [],
    };
  }

  // 模块子树记忆化：同一模块在多处被依赖时复用同一子树实例。
  const moduleCache = new Map<number, TreeNode>();

  return buildModuleNode(rootId, "app", new Set());

  function buildModuleNode(
    projectId: number,
    kind: "app" | "module",
    pathAncestors: Set<number>,
  ): TreeNode {
    const cached = moduleCache.get(projectId);
    if (cached) return cached;

    const project = projectById.get(projectId)!;
    const children: TreeNode[] = [];
    const seenLibKeys = new Set<string>();
    const nextAncestors = new Set(pathAncestors).add(projectId);

    for (const e of edgesByFrom.get(projectId) ?? []) {
      const upstream = e.sourceProjectId;
      if (upstream != null && closureIds.has(upstream)) {
        // workspace 模块子依赖：环（祖先路径上）直接跳过；菱形共享走缓存。
        if (pathAncestors.has(upstream)) continue;
        children.push(buildModuleNode(upstream, "module", nextAncestors));
      } else {
        // 库依赖叶子（local / remote；workspace 命中但不在闭包内也按库展示）。
        const key = `${e.dependency.groupId}:${e.dependency.artifactId}:${e.dependency.version ?? ""}:${e.source}`;
        if (seenLibKeys.has(key)) continue;
        seenLibKeys.add(key);
        const source: DependencyNodeSource =
          e.source === "workspaceSource" ? "localRepository" : e.source;
        children.push({
          key: `lib:${projectId}:${key}`,
          kind: "library",
          projectId: null,
          edge: e,
          label: e.dependency.artifactId,
          coordinates: coords(e.dependency),
          version: e.dependency.version ?? null,
          path: e.resolvedPath,
          source,
          children: [],
        });
      }
    }

    const node: TreeNode = {
      key: `mod:${projectId}`,
      kind,
      projectId,
      edge: null,
      label: project.coordinates.artifactId,
      coordinates: coords(project.coordinates),
      version: project.coordinates.version ?? null,
      path: project.path,
      source: "workspaceSource",
      children,
    };
    moduleCache.set(projectId, node);
    return node;
  }
}

/** 按模块名 / 坐标过滤子图：保留命中节点及其祖先与后代。空查询原样返回。 */
export function filterTree(root: TreeNode, query: string): TreeNode {
  const q = query.trim().toLowerCase();
  if (!q) return root;
  return filterNode(root)!;

  function matches(n: TreeNode): boolean {
    return (
      n.label.toLowerCase().includes(q) || n.coordinates.toLowerCase().includes(q)
    );
  }

  function filterNode(n: TreeNode): TreeNode | null {
    const keptChildren = n.children
      .map(filterNode)
      .filter((c): c is TreeNode => c != null);
    if (matches(n) || keptChildren.length > 0) {
      return keptChildren.length === n.children.length ? n : { ...n, children: keptChildren };
    }
    return null;
  }
}

/** 展开态：节点 key 集合（在集合内 = 已展开显示 children；共享子树全局一致）。 */
export type ExpandedSet = Set<string>;

/** 默认展开策略：app 与一层模块展开，库依赖层默认折叠（大图性能）。 */
export function defaultExpanded(root: TreeNode): ExpandedSet {
  const set: ExpandedSet = new Set([root.key]);
  for (const child of root.children) {
    if (child.kind === "module") set.add(child.key);
  }
  return set;
}

/** 全部展开（不含叶子）。注意：大图（共享子树）下慎用——以可见行数为准。 */
export function expandAll(root: TreeNode): ExpandedSet {
  const set: ExpandedSet = new Set();
  walk(root, new Set());
  return set;
  function walk(n: TreeNode, visited: Set<string>) {
    if (n.children.length > 0 && !visited.has(n.key)) {
      visited.add(n.key);
      set.add(n.key);
      for (const c of n.children) walk(c, visited);
    }
  }
}

export interface FlatRow {
  /** 路径感知的唯一 key（共享子树多处出现时行不冲突；虚拟列表 item-key）。 */
  key: string;
  node: TreeNode;
  depth: number;
  /** 是否有子节点（渲染展开箭头）。 */
  hasChildren: boolean;
}

/** 展平当前可见节点（折叠 + 虚拟化渲染的基础）：只含展开节点的直接子层。 */
export function flattenVisible(root: TreeNode, expanded: ExpandedSet): FlatRow[] {
  const rows: FlatRow[] = [];
  walk(root, 0, "mod-tree");
  return rows;

  function walk(n: TreeNode, depth: number, pathKey: string) {
    rows.push({ key: pathKey, node: n, depth, hasChildren: n.children.length > 0 });
    if (expanded.has(n.key)) {
      for (const c of n.children) walk(c, depth + 1, `${pathKey}/${c.key}`);
    }
  }
}

/** 去重统计（共享子树只计一次）：模块数 + 库依赖边数。 */
export function countUniqueNodes(root: TreeNode): { modules: number; libraries: number } {
  const seenModules = new Set<string>();
  const seenLibs = new Set<string>();
  walk(root);
  return { modules: seenModules.size, libraries: seenLibs.size };
  function walk(n: TreeNode) {
    if (n.kind !== "library") {
      if (seenModules.has(n.key)) return;
      seenModules.add(n.key);
    } else {
      seenLibs.add(n.key);
      return;
    }
    for (const c of n.children) walk(c);
  }
}
