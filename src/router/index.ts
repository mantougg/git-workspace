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
  ],
});

export default router;
