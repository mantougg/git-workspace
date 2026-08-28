import { useRoute } from "vue-router";
import { useRepositoryStore } from "@/stores/repository";
import { useWorkspaceStore } from "@/stores/workspace";

// 路径比较前两侧归一化分隔符（Windows 混合分隔符，见 AGENTS.md 平台规范）；
// 返回值仍用原始路径，归一化只用于匹配。
const norm = (p: string) => p.replace(/\\/g, "/");

/**
 * F-17：Git 视图的「当前仓库」统一解析（SideNav 直达场景）。
 *
 * 解析顺序：
 * 1. `route.query.repo`（变更页等带参跳转，优先级最高），命中回写 store；
 * 2. `repoStore.currentRepoPath`（全局当前仓库，变更页勾选同步写入），
 *    需仍属于当前工作区（防止切换工作区后用到旧仓库）；
 * 3. 当前工作区仓库列表第一个（兜底；列表缺失/属其他工作区时先拉取）。
 *
 * 返回空串表示当前没有工作区或工作区内没有任何仓库，由调用方提示。
 */
export function useCurrentRepo() {
  const route = useRoute();
  const repoStore = useRepositoryStore();
  const workspaceStore = useWorkspaceStore();

  async function resolveCurrentRepo(): Promise<string> {
    const fromQuery = route.query.repo;
    if (typeof fromQuery === "string" && fromQuery) {
      repoStore.setCurrentRepoPath(fromQuery);
      return fromQuery;
    }

    if (!workspaceStore.currentWorkspace) {
      await workspaceStore.loadWorkspaces();
    }
    const wsId = workspaceStore.currentWorkspace?.id;
    if (wsId == null) return "";

    if (repoStore.repositoriesWorkspaceId !== wsId) {
      await repoStore.loadRepositories(wsId);
    }
    const paths = repoStore.repositories.map((r) => r.repository.path);

    const current = repoStore.currentRepoPath;
    if (current && paths.some((p) => norm(p) === norm(current))) {
      return current;
    }

    const first = paths[0] ?? "";
    if (first) {
      repoStore.setCurrentRepoPath(first);
    }
    return first;
  }

  return { resolveCurrentRepo };
}
