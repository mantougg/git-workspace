import { createRouter, createWebHistory } from "vue-router";

const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: "/",
      name: "repository-list",
      component: () => import("@/views/RepositoryList.vue"),
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
  ],
});

export default router;
