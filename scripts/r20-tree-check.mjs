// R-20 验收辅助脚本（不进测试套件，手动运行）：
//   node scripts/r20-tree-check.mjs
// 1) §45 示例拓扑正确渲染断言（boot → web → { auth → core → common, system }
//    + spring-boot），三种来源可区分；
// 2) 100+ 模块场景的树组装 / 过滤 / 展平耗时测量（渲染预算证据）。
import { buildDependencyTree, filterTree, flattenVisible, countUniqueNodes } from "../src/utils/dependencyTree.ts";

let failures = 0;
function assert(cond, msg) {
  if (cond) {
    console.log("  ✓", msg);
  } else {
    failures++;
    console.error("  ✗", msg);
  }
}

// ---- §45 拓扑 ----
const mk = (id, artifactId) => ({
  projectId: id,
  repositoryId: 1,
  path: `/ws/repo/${artifactId}/pom.xml`,
  coordinates: { groupId: "com.example", artifactId, version: "1.0.0" },
  packaging: "jar",
  pomHash: "h" + id,
});
const projects = [
  mk(1, "boot"), mk(2, "web"), mk(3, "auth"), mk(4, "core"), mk(5, "common"),
  mk(6, "system"), mk(7, "spring-boot"),
];
const makeDep = (projectList) => (from, to, source, sourceProjectId, resolvedPath) => ({
  dependencyId: from * 100 + to,
  fromProjectId: from,
  dependency: {
    groupId: sourceProjectId ? "com.example" : "org.lib",
    artifactId: sourceProjectId ? projectList[to - 1].coordinates.artifactId : `lib${to}`,
    version: "1.0.0",
    scope: "compile",
    optional: false,
    depType: "jar",
    classifier: null,
    exclusions: [],
  },
  source,
  sourceProjectId,
  resolvedPath: resolvedPath ?? null,
  reason: source === "workspaceSource" ? "workspaceExactMatch" : source === "localRepository" ? "localArtifactExists" : "remoteArtifactMissingLocally",
});
const dep = makeDep(projects);
const dependencies = [
  dep(1, 2, "workspaceSource", 2),        // boot → web
  dep(2, 3, "workspaceSource", 3),        // web → auth
  dep(3, 4, "workspaceSource", 4),        // auth → core
  dep(4, 5, "workspaceSource", 5),        // core → common
  dep(2, 6, "workspaceSource", 6),        // web → system
  dep(1, 7, "workspaceSource", 7),        // boot → spring-boot
  dep(1, 101, "localRepository", null, "/home/u/.m2/repository/org/lib/lib101/1.0.0.jar"),
  dep(2, 102, "remoteRepository", null, null), // 远程缺失
];
const graph = {
  workspaceId: 1, fingerprint: "f", projects, dependencies, modules: [],
  sourceMappings: [], totalDependencies: dependencies.length, truncated: false,
};
const closure = {
  workspaceId: 1, rootProjectId: 1, graphFingerprint: "f", mode: "auto",
  projects,
};

console.log("§45 拓扑渲染（数据组装）：");
const root = buildDependencyTree(graph, closure);
assert(root.kind === "app" && root.label === "boot", "根节点为应用 boot");
const childLabels = root.children.map((c) => c.label).sort();
assert(
  JSON.stringify(childLabels) === JSON.stringify(["lib101", "spring-boot", "web"]),
  "boot 直接子节点 = web + spring-boot + 本地库 lib101",
);
const web = root.children.find((c) => c.label === "web");
assert(
  JSON.stringify(web.children.map((c) => c.label).sort()) ===
    JSON.stringify(["auth", "lib102", "system"]),
  "web 子节点 = auth + system + 远程库 lib102",
);
const auth = web.children.find((c) => c.label === "auth");
const core = auth.children.find((c) => c.label === "core");
const common = core.children.find((c) => c.label === "common");
assert(!!common, "auth → core → common 链正确");
const localLeaf = root.children.find((c) => c.label === "lib101");
assert(localLeaf && localLeaf.source === "localRepository" && localLeaf.path, "本地库叶子带解析路径");
const remoteLeaf = web.children.find((c) => c.source === "remoteRepository");
assert(!!remoteLeaf, "远程库叶子可区分");
const sources = new Set([root.source, localLeaf.source, remoteLeaf.source]);
assert(
  sources.has("workspaceSource") && sources.has("localRepository") && sources.has("remoteRepository"),
  "三种来源状态可区分",
);

// 过滤
const filtered = filterTree(root, "common");
const flat = flattenVisible(filtered, new Set(["mod:1", "mod:2", "mod:3", "mod:4", "mod:5"]));
assert(flat.some((r) => r.node.label === "common"), "搜索 common 命中且保留祖先链");

console.log("\n100+ 模块大图测量：");
const N = 120, LIBS_PER_MODULE = 30;
const bigProjects = Array.from({ length: N + 1 }, (_, i) => mk(i + 1, i === 0 ? "app" : `mod${i}`));
const bigDeps = [];
// app → 所有模块；mod_i → mod_{i+1}（链）；每模块 30 个库依赖（local/remote 对半）。
const bdep = makeDep(bigProjects);
for (let i = 1; i <= N; i++) bigDeps.push(bdep(1, i, "workspaceSource", i));
for (let i = 1; i < N; i++) bigDeps.push(bdep(i + 1, i + 2, "workspaceSource", i + 2));
for (let i = 1; i <= N; i++) {
  for (let j = 0; j < LIBS_PER_MODULE; j++) {
    const libId = 1000 + i * 100 + j;
    bigDeps.push(
      j % 2 === 0
        ? bdep(i + 1, libId, "localRepository", null, `/m2/lib${libId}.jar`)
        : bdep(i + 1, libId, "remoteRepository", null, null),
    );
  }
}
const bigGraph = {
  workspaceId: 1, fingerprint: "f2", projects: bigProjects, dependencies: bigDeps,
  modules: [], sourceMappings: [], totalDependencies: bigDeps.length, truncated: false,
};
const bigClosure = {
  workspaceId: 1, rootProjectId: 1, graphFingerprint: "f2", mode: "auto",
  projects: bigProjects,
};

let t0 = performance.now();
const bigRoot = buildDependencyTree(bigGraph, bigClosure);
const buildMs = performance.now() - t0;
const { modules, libraries } = countUniqueNodes(bigRoot);
t0 = performance.now();
const expandedAll = new Set(bigProjects.map((p) => `mod:${p.projectId}`));
expandedAll.add(bigRoot.key);
const rows = flattenVisible(bigRoot, expandedAll);
const flattenMs = performance.now() - t0;
t0 = performance.now();
filterTree(bigRoot, "mod99");
const filterMs = performance.now() - t0;

console.log(`  模块数: ${modules}，库依赖边: ${libraries}`);
console.log(`  树组装 buildDependencyTree: ${buildMs.toFixed(2)} ms`);
console.log(`  全展开展平 flattenVisible: ${flattenMs.toFixed(2)} ms（${rows.length} 可见行，虚拟化渲染仅取视口内行）`);
console.log(`  搜索过滤 filterTree: ${filterMs.toFixed(2)} ms`);
console.log(
  `  UI 渲染预算口径：可见行数由折叠控制（默认仅展开一层，可见 ≤ ${N + 1 + 1} 行），` +
    `n-virtual-list 虚拟化后 DOM 节点与视口内行数成正比（约 34px × 视口行数），与总数无关。`,
);

if (failures > 0) {
  console.error(`\n${failures} 个断言失败`);
  process.exit(1);
}
console.log("\n全部断言通过 ✓");
