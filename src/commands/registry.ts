/**
 * 命令注册表（T-31）
 * 每条命令 = id + title + group + run()
 * 只编排已有能力，不新增业务逻辑。
 *
 * 命令上下文（router / stores）由 setup 期构建并显式传入：
 * 快捷键监听在 keydown 事件上下文里执行，拿不到组件实例，
 * useRouter() / useXxxStore() 必须在 setup 期解析好（D-14 隐患修复）。
 */

import type { Router } from "vue-router";
import type { useWorkspaceStore } from "@/stores/workspace";
import type { useRepositoryStore } from "@/stores/repository";
import type { useAiStore } from "@/stores/ai";
import { openInTerminal, openInIde, type TerminalKind, type IdeKind } from "@/api/integration";

export interface Command {
  id: string;
  title: string;
  group: string;
  /** 快捷键描述（展示用，如 "Ctrl+K"） */
  shortcut?: string;
  run: () => void | Promise<void>;
}

/** 命令运行所需的上下文；App.vue / CommandPalette 在 setup 期构建。 */
export interface CommandContext {
  router: Router;
  workspaceStore: ReturnType<typeof useWorkspaceStore>;
  repoStore: ReturnType<typeof useRepositoryStore>;
  aiStore: ReturnType<typeof useAiStore>;
}

/** 提交请求事件（Ctrl+Enter / Ctrl+Shift+Enter → 变更页提交面板）。 */
export const COMMIT_REQUEST_EVENT = "gw:commit-request";

export function requestCommit(push: boolean): void {
  window.dispatchEvent(
    new CustomEvent(COMMIT_REQUEST_EVENT, { detail: { push } })
  );
}

/** 打开目标：优先当前仓库，缺省回落当前工作区根目录。 */
function currentTargetPath(ctx: CommandContext): string {
  return (
    ctx.repoStore.currentRepoPath ||
    ctx.workspaceStore.currentWorkspace?.path ||
    ""
  );
}

/** 从 router meta 提取导航命令 */
function getNavigationCommands(ctx: CommandContext): Command[] {
  const routes = ctx.router.getRoutes();

  return routes
    .filter((r) => r.meta.nav !== false && r.name)
    .map((r) => ({
      id: `nav:${String(r.name)}`,
      title: `打开: ${r.meta.title ?? String(r.name)}`,
      group: "导航",
      run: () => {
        ctx.router.push({ name: r.name as string });
      },
    }));
}

/** 高频操作命令 */
function getActionCommands(ctx: CommandContext): Command[] {
  const { router, workspaceStore, repoStore, aiStore } = ctx;

  return [
    {
      id: "action:toggle-assistant",
      title: "切换 AI 助手抽屉",
      group: "操作",
      run: () => {
        aiStore.toggleDrawer();
      },
    },
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
      id: "action:repo-search",
      title: "搜索文件或仓库（变更页）",
      group: "操作",
      // Ctrl+Shift+F 暂绑同一入口：FTS5 代码搜索（ai_search）有后端无 UI，
      // 代码搜索视图随 T-28 落地后把该快捷键切换到专用命令。
      run: () => {
        router.push({ name: "changes", query: { focus: "search" } });
      },
    },
    {
      id: "action:refresh",
      title: "刷新当前窗口",
      group: "操作",
      run: () => {
        window.location.reload();
      },
    },
  ];
}

/** Git 操作命令（变更页 action 通道 / 对应 Git 视图直达）。 */
function getGitCommands(ctx: CommandContext): Command[] {
  const { router } = ctx;

  const toChanges = (
    action: string,
    selector: string | null,
    title: string
  ): Command => ({
    id: `git:${action}`,
    title,
    group: "Git 操作",
    run: () => {
      router.push({
        name: "changes",
        query: selector ? { selector, action } : { action },
      });
    },
  });

  return [
    toChanges("fetch", "@status:clean", "Fetch 全部仓库"),
    toChanges("pull", "@status:clean", "Pull Clean 仓库"),
    toChanges("push", "@status:ahead", "Push Ahead 仓库"),
    toChanges("commit", "@status:dirty", "Commit 有变更仓库"),
    {
      id: "action:commit",
      title: "提交当前勾选变更（提交面板）",
      group: "Git 操作",
      run: () => {
        requestCommit(false);
      },
    },
    {
      id: "action:commit-push",
      title: "提交并推送当前勾选变更（提交面板）",
      group: "Git 操作",
      run: () => {
        requestCommit(true);
      },
    },
    toChanges("sync", null, "Sync 全部仓库（Fetch + Pull Clean）"),
    toChanges("branch-create", "@status:clean", "新建分支（变更页批量）"),
    {
      id: "git:branch",
      title: "打开分支管理（checkout / merge / rebase）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "branch-manager" });
      },
    },
    {
      id: "git:stash",
      title: "打开 Stash 管理",
      group: "Git 操作",
      run: () => {
        router.push({ name: "stash-manager" });
      },
    },
    {
      id: "git:reset",
      title: "打开提交图（Reset / Cherry-pick / Revert）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "git-graph" });
      },
    },
    {
      id: "git:reflog",
      title: "打开 Reflog",
      group: "Git 操作",
      run: () => {
        router.push({ name: "reflog-view" });
      },
    },
    {
      id: "git:worktree",
      title: "打开 Worktree 管理",
      group: "Git 操作",
      run: () => {
        router.push({ name: "worktree-manager" });
      },
    },
    {
      id: "git:ai-review",
      title: "AI Review（打开 AI 助手）",
      group: "Git 操作",
      run: () => {
        ctx.aiStore.toggleDrawer();
      },
    },
  ];
}

/** 终端命令（平台专属类型仅 Windows 列出；不可用时报可行动错误）。 */
function getTerminalCommands(ctx: CommandContext): Command[] {
  const kindTitles: Array<[TerminalKind, string]> =
    window.navigator.platform.toLowerCase().includes("win")
      ? [
          ["system", "在默认终端打开当前仓库"],
          ["powershell", "在 PowerShell 打开当前仓库"],
          ["cmd", "在 CMD 打开当前仓库"],
          ["git-bash", "在 Git Bash 打开当前仓库"],
          ["windows-terminal", "在 Windows Terminal 打开当前仓库"],
        ]
      : [["system", "在终端打开当前仓库"]];

  return kindTitles.map(([kind, title]) => ({
    id: `terminal:${kind}`,
    title,
    group: "终端",
    run: async () => {
      const path = currentTargetPath(ctx);
      if (!path) {
        throw new Error("当前没有选中的仓库或工作区，无法打开终端");
      }
      await openInTerminal(path, kind);
    },
  }));
}

/** IDE 命令（VS Code / IntelliJ IDEA / Cursor / Zed）。 */
function getIdeCommands(ctx: CommandContext): Command[] {
  const ides: Array<[IdeKind, string]> = [
    ["vscode", "VS Code"],
    ["idea", "IntelliJ IDEA"],
    ["cursor", "Cursor"],
    ["zed", "Zed"],
  ];

  return ides.map(([ide, label]) => ({
    id: `ide:${ide}`,
    title: `在 ${label} 打开当前仓库`,
    group: "IDE",
    run: async () => {
      const path = currentTargetPath(ctx);
      if (!path) {
        throw new Error("当前没有选中的仓库或工作区，无法打开 IDE");
      }
      await openInIde(path, ide);
    },
  }));
}

/** 获取所有命令（导航 + 操作 + Git 操作 + 终端 + IDE） */
export function getAllCommands(ctx: CommandContext): Command[] {
  return [
    ...getNavigationCommands(ctx),
    ...getActionCommands(ctx),
    ...getGitCommands(ctx),
    ...getTerminalCommands(ctx),
    ...getIdeCommands(ctx),
  ];
}
