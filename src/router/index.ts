import { createRouter, createWebHistory } from "vue-router";

export type NavGroup = "工作区" | "Git" | "Runtime" | "设置" | "无";

declare module "vue-router" {
  interface RouteMeta {
    /** SideNav 分组 */
    group?: NavGroup;
    /** 显示标题 */
    title?: string;
    /** 是否进 SideNav（默认 true，任务型页面 false） */
    nav?: boolean;
  }
}

const router = createRouter({
  history: createWebHistory(),
  routes: [
    // ── 工作区 ──────────────────────────────────────────────
    {
      path: "/",
      name: "dashboard",
      component: () => import("@/views/DashboardView.vue"),
      meta: { group: "工作区", title: "总览" },
    },
    {
      path: "/changes",
      name: "changes",
      component: () => import("@/views/RepositoryList.vue"),
      meta: { group: "工作区", title: "变更与批量操作" },
    },
    {
      path: "/health",
      name: "health",
      component: () => import("@/views/HealthView.vue"),
      meta: { group: "工作区", title: "健康检查" },
    },
    // ── Git ─────────────────────────────────────────────────
    {
      path: "/graph",
      name: "git-graph",
      component: () => import("@/views/GitGraph.vue"),
      meta: { group: "Git", title: "提交图" },
    },
    {
      path: "/branches",
      name: "branch-manager",
      component: () => import("@/views/BranchManager.vue"),
      meta: { group: "Git", title: "分支" },
    },
    {
      path: "/stash",
      name: "stash-manager",
      component: () => import("@/views/StashManager.vue"),
      meta: { group: "Git", title: "Stash" },
    },
    {
      path: "/worktrees",
      name: "worktree-manager",
      component: () => import("@/views/WorktreeManager.vue"),
      meta: { group: "Git", title: "Worktree" },
    },
    {
      path: "/symbols",
      name: "symbol-search",
      component: () => import("@/views/SymbolSearchView.vue"),
      meta: { group: "Git", title: "符号" },
    },
    {
      path: "/reflog",
      name: "reflog-view",
      component: () => import("@/views/Reflog.vue"),
      meta: { group: "Git", title: "Reflog" },
    },
    {
      path: "/change-sets",
      name: "change-sets",
      component: () => import("@/views/ChangeSetView.vue"),
      meta: { group: "Git", title: "Change Set" },
    },
    {
      path: "/pipeline",
      name: "pipeline",
      component: () => import("@/views/PipelineView.vue"),
      meta: { group: "Git", title: "Pipeline" },
    },
    {
      path: "/manifest",
      name: "manifest",
      component: () => import("@/views/ManifestView.vue"),
      meta: { group: "Git", title: "Manifest" },
    },
    {
      path: "/operation-log",
      name: "operation-log",
      component: () => import("@/views/OperationLogView.vue"),
      meta: { group: "Git", title: "操作日志" },
    },
    // ── Runtime ─────────────────────────────────────────────
    {
      path: "/runtime",
      name: "runtime-dashboard",
      component: () => import("@/views/RuntimeDashboard.vue"),
      meta: { group: "Runtime", title: "Runtime 总览" },
    },
    {
      path: "/runtime/dependencies",
      name: "runtime-dependencies",
      component: () => import("@/views/RuntimeDependenciesView.vue"),
      meta: { group: "Runtime", title: "依赖" },
    },
    {
      path: "/runtime/scope",
      name: "runtime-scope",
      component: () => import("@/views/RuntimeScopeView.vue"),
      meta: { group: "Runtime", title: "作用域" },
    },
    {
      path: "/runtime/logs",
      name: "runtime-logs",
      component: () => import("@/views/RuntimeLogsView.vue"),
      meta: { group: "Runtime", title: "日志" },
    },
    {
      path: "/runtime/environments",
      name: "runtime-environments",
      component: () => import("@/views/RuntimeEnvironmentsView.vue"),
      meta: { group: "Runtime", title: "多服务环境" },
    },
    // ── 设置 ────────────────────────────────────────────────
    {
      path: "/workspaces",
      name: "workspaces",
      component: () => import("@/views/WorkspaceManageView.vue"),
      meta: { group: "设置", title: "工作区管理" },
    },
    {
      path: "/jdk-manager",
      name: "jdk-manager",
      component: () => import("@/views/JdkManagerView.vue"),
      meta: { group: "设置", title: "JDK 管理" },
    },
    {
      path: "/maven-settings",
      name: "maven-settings",
      component: () => import("@/views/MavenSettingsView.vue"),
      meta: { group: "设置", title: "Maven 设置" },
    },
    {
      path: "/node-toolchain",
      name: "node-toolchain",
      component: () => import("@/views/NodeToolchainView.vue"),
      meta: { group: "设置", title: "Node 工具链" },
    },
    {
      path: "/port-tool",
      name: "port-tool",
      component: () => import("@/views/PortToolView.vue"),
      meta: { group: "设置", title: "端口工具" },
    },
    {
      path: "/ai-settings",
      name: "ai-settings",
      component: () => import("@/views/AiSettingsView.vue"),
      meta: { group: "设置", title: "AI 设置" },
    },
    {
      path: "/about",
      name: "about",
      component: () => import("@/views/AboutView.vue"),
      meta: { group: "设置", title: "关于" },
    },
    // ── 任务型页面（不进 SideNav） ──────────────────────────
    {
      path: "/diff",
      name: "diff-viewer",
      component: () => import("@/views/DiffViewer.vue"),
      meta: { group: "无", title: "Diff", nav: false },
    },
    {
      path: "/conflicts",
      name: "conflict-resolver",
      component: () => import("@/views/ConflictResolver.vue"),
      meta: { group: "无", title: "冲突解决", nav: false },
    },
    {
      path: "/runtime/app-wizard",
      name: "runtime-app-wizard",
      component: () => import("@/views/RuntimeAppWizard.vue"),
      meta: { group: "无", title: "新建应用", nav: false },
    },
  ],
});

export default router;
