/**
 * 命令注册表
 * 每条命令 = id + title + group + run()
 * 只编排已有能力，不新增业务逻辑。
 */

import { useRouter } from "vue-router";
import { useWorkspaceStore } from "@/stores/workspace";
import { useRepositoryStore } from "@/stores/repository";

export interface Command {
  id: string;
  title: string;
  group: string;
  /** 快捷键描述（展示用，如 "Ctrl+K"） */
  shortcut?: string;
  run: () => void | Promise<void>;
}

/** 从 router meta 提取导航命令 */
function getNavigationCommands(): Command[] {
  const router = useRouter();
  const routes = router.getRoutes();

  return routes
    .filter((r) => r.meta.nav !== false && r.name)
    .map((r) => ({
      id: `nav:${String(r.name)}`,
      title: `打开: ${r.meta.title ?? String(r.name)}`,
      group: "导航",
      run: () => {
        router.push({ name: r.name as string });
      },
    }));
}

/** 高频操作命令 */
function getActionCommands(): Command[] {
  const router = useRouter();
  const workspaceStore = useWorkspaceStore();
  const repoStore = useRepositoryStore();

  return [
    {
      id: "action:scan",
      title: "扫描仓库",
      group: "操作",
      run: async () => {
        const wsId = workspaceStore.currentWorkspace?.id;
        if (wsId) {
          await repoStore.scanRepositories(wsId);
        }
      },
    },
    {
      id: "action:fetch-all",
      title: "Fetch 全部仓库",
      group: "操作",
      run: () => {
        router.push({ name: "changes", query: { selector: "@status:clean", action: "fetch" } });
      },
    },
    {
      id: "action:pull-clean",
      title: "Pull Clean 仓库",
      group: "操作",
      run: () => {
        router.push({ name: "changes", query: { selector: "@status:clean", action: "pull" } });
      },
    },
    {
      id: "action:push-ahead",
      title: "Push Ahead 仓库",
      group: "操作",
      run: () => {
        router.push({ name: "changes", query: { selector: "@status:ahead", action: "push" } });
      },
    },
    {
      id: "action:commit-dirty",
      title: "Commit 有变更仓库",
      group: "操作",
      run: () => {
        router.push({ name: "changes", query: { selector: "@status:dirty", action: "commit" } });
      },
    },
    {
      id: "action:health-check",
      title: "运行健康检查",
      group: "操作",
      run: () => {
        router.push({ name: "health" });
      },
    },
    {
      id: "action:new-change-set",
      title: "新建 Change Set",
      group: "操作",
      run: () => {
        router.push({ name: "change-sets" });
      },
    },
    {
      id: "action:workspace-manage",
      title: "管理工作区",
      group: "操作",
      run: () => {
        router.push({ name: "workspaces" });
      },
    },
  ];
}

/** 获取所有命令（导航 + 操作） */
export function getAllCommands(): Command[] {
  return [...getNavigationCommands(), ...getActionCommands()];
}
