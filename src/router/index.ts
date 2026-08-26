import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      name: "dashboard",
      component: () => import("@/views/DashboardView.vue"),
    },
    {
      path: "/changes",
      name: "changes",
      component: () => import("@/views/RepositoryList.vue"),
    },
    {
      path: "/health",
      name: "health",
      component: () => import("@/views/HealthView.vue"),
    },
    {
      path: "/change-sets",
      name: "change-sets",
      component: () => import("@/views/ChangeSetView.vue"),
    },
    {
      path: "/pipeline",
      name: "pipeline",
      component: () => import("@/views/PipelineView.vue"),
    },
    {
      path: "/manifest",
      name: "manifest",
      component: () => import("@/views/ManifestView.vue"),
    },
    {
      path: "/operation-log",
      name: "operation-log",
      component: () => import("@/views/OperationLogView.vue"),
    },
    {
      path: "/diff",
      name: "diff-viewer",
      component: () => import("@/views/DiffViewer.vue"),
    },
    {
      path: "/graph",
      name: "git-graph",
      component: () => import("@/views/GitGraph.vue"),
    },
    {
      path: "/branches",
      name: "branch-manager",
      component: () => import("@/views/BranchManager.vue"),
    },
    {
      path: "/reflog",
      name: "reflog-view",
      component: () => import("@/views/Reflog.vue"),
    },
    {
      path: "/worktrees",
      name: "worktree-manager",
      component: () => import("@/views/WorktreeManager.vue"),
    },
    {
      path: "/stash",
      name: "stash-manager",
      component: () => import("@/views/StashManager.vue"),
    },
    {
      path: "/conflicts",
      name: "conflict-resolver",
      component: () => import("@/views/ConflictResolver.vue"),
    },
    {
      path: "/jdk-manager",
      name: "jdk-manager",
      component: () => import("@/views/JdkManagerView.vue"),
    },
    {
      path: "/maven-settings",
      name: "maven-settings",
      component: () => import("@/views/MavenSettingsView.vue"),
    },
    // ── R-13 Runtime Workspace ──────────────────────────────────────
    {
      path: "/runtime",
      name: "runtime-dashboard",
      component: () => import("@/views/RuntimeDashboard.vue"),
    },
    {
      path: "/runtime/app-wizard",
      name: "runtime-app-wizard",
      component: () => import("@/views/RuntimeAppWizard.vue"),
    },
    {
      path: "/runtime/dependencies",
      name: "runtime-dependencies",
      component: () => import("@/views/RuntimeDependenciesView.vue"),
    },
    {
      path: "/runtime/scope",
      name: "runtime-scope",
      component: () => import("@/views/RuntimeScopeView.vue"),
    },
    {
      path: "/runtime/logs",
      name: "runtime-logs",
      component: () => import("@/views/RuntimeLogsView.vue"),
    },
  ],
});

export default router;
