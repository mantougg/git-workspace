/**
 * AI-10：领域页面接入统一 Assistant 的 facade（设计文档 §9.1 / §12.3）。
 *
 * 视图只负责两件事：
 * 1. 提供上下文入口（`openAssistant` 带入作用域/补充上下文/推断角色）；
 * 2. 专用快捷动作（现有 AiGitAssistantDialog 等场景对话框保持原样）。
 * 会话状态全局唯一（stores/ai.ts），视图不各自实现聊天状态。
 */

import { computed } from "vue";
import { useRoute } from "vue-router";
import { useAiStore, type OpenAssistantOptions } from "@/stores/ai";
import { useWorkspaceStore } from "@/stores/workspace";
import type { AiSessionRole } from "@/types/ai";

/** §9.2 七个受限角色的展示名。 */
export const ROLE_LABELS: Record<AiSessionRole, string> = {
  workspaceAssistant: "Workspace Assistant",
  gitReviewer: "Git Reviewer",
  commitAssistant: "Commit Assistant",
  conflictAssistant: "Conflict Assistant",
  runtimeDiagnostician: "Runtime Diagnostician",
  runtimeConfigAdvisor: "Runtime Config Advisor",
  actionPlanner: "Action Planner",
};

/** 路由 → 角色自动推断（§9.2：入口自动推断 + 结果 UI 可见）。 */
const ROUTE_ROLE_MAP: Record<string, AiSessionRole> = {
  dashboard: "workspaceAssistant",
  workspaces: "workspaceAssistant",
  health: "workspaceAssistant",
  changes: "gitReviewer",
  "git-graph": "gitReviewer",
  "branch-manager": "gitReviewer",
  "change-sets": "commitAssistant",
  "diff-viewer": "gitReviewer",
  "conflict-resolver": "conflictAssistant",
  "runtime-dashboard": "runtimeDiagnostician",
  "runtime-logs": "runtimeDiagnostician",
  "runtime-scope": "runtimeConfigAdvisor",
  "runtime-environments": "runtimeConfigAdvisor",
  "runtime-dependencies": "runtimeConfigAdvisor",
};

export function inferRoleForRoute(routeName: string | null | undefined): AiSessionRole {
  return (routeName && ROUTE_ROLE_MAP[routeName]) || "workspaceAssistant";
}

export function useAiAssistant() {
  const ai = useAiStore();
  const route = useRoute();
  const workspaceStore = useWorkspaceStore();

  /** 打开 Drawer 并带入上下文；缺省用当前工作区与路由推断角色。 */
  function openAssistant(options: OpenAssistantOptions = {}) {
    const workspace = workspaceStore.currentWorkspace;
    ai.openWithContext({
      ...options,
      workspaceId: options.workspaceId ?? workspace?.id ?? null,
      workspaceName: options.workspaceName ?? workspace?.name ?? null,
      inferredRole:
        options.inferredRole ??
        inferRoleForRoute(typeof route.name === "string" ? route.name : null),
    });
  }

  /** 作用域展示（§9.1：「当前工作区 / 3 个仓库 / Runtime gateway / 选中日志 86 行」）。 */
  const scopeLabel = computed(() => {
    const parts: string[] = [];
    const scope = ai.scope;
    if (scope.workspaceName) parts.push(scope.workspaceName);
    else if (scope.workspaceId != null) parts.push(`工作区 #${scope.workspaceId}`);
    if (scope.repositoryPaths.length > 0) {
      parts.push(
        scope.repositoryPaths.length === 1
          ? shortPath(scope.repositoryPaths[0])
          : `${scope.repositoryPaths.length} 个仓库`,
      );
    }
    if (scope.runtimeName) {
      parts.push(
        scope.processId != null
          ? `Runtime ${scope.runtimeName} · 进程 #${scope.processId}`
          : `Runtime ${scope.runtimeName}`,
      );
    }
    if (scope.origin) parts.push(scope.origin);
    return parts.length > 0 ? parts.join(" / ") : "未选择上下文";
  });

  return {
    ai,
    openAssistant,
    scopeLabel,
    roleLabels: ROLE_LABELS,
    inferRoleForRoute,
  };
}

function shortPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const segments = normalized.split("/").filter(Boolean);
  return segments[segments.length - 1] ?? normalized;
}
