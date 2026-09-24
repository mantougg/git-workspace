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
import { openInTerminal, openInIde, IDE_DISPLAY_NAMES, type TerminalKind, type IdeKind } from "@/api/integration";
import { encodeUtf8Base64 } from "@/utils/base64";

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
  const { router, workspaceStore, repoStore } = ctx;

  /** GF-18：作用于「当前仓库」的命令共用守卫（无仓库时抛错，由命令面板展示）。 */
  function requireCurrentRepo(): string {
    const path = repoStore.currentRepoPath;
    if (!path) {
      throw new Error("当前没有选中的仓库，请先在变更页选择仓库");
    }
    return path;
  }

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
    // GF-03：diff-viewer / conflict-resolver 是 nav:false 任务型路由，被
    // getNavigationCommands 过滤掉（`nav:<name>` 命令从未注册，Ctrl+Shift+D
    // 此前绑死在死命令上）。这里按 Route 显式注册，不进 SideNav。
    {
      id: "git:diff",
      title: "打开 Diff 视图",
      group: "Git 操作",
      run: () => {
        // DiffViewer onMounted 走 resolveCurrentRepo（query.repo → 全局当前
        // 仓库 → 工作区首仓库兜底，F-14/F-17），无参直达有兜底不会警告。
        router.push({ name: "diff-viewer" });
      },
    },
    {
      id: "git:open-conflict-resolver",
      title: "打开冲突解决器",
      group: "Git 操作",
      run: () => {
        // 与 RepositoryList「冲突」入口同参：ConflictResolver 需要
        // workspace（队列模式扫全部冲突仓库）或 repo 参数，无参直达会
        // warning 并回变更页。无工作区上下文时回变更页。
        const ws = workspaceStore.currentWorkspace;
        if (!ws) {
          router.push({ name: "changes" });
          return;
        }
        router.push({
          name: "conflict-resolver",
          query: { workspace: String(ws.id), name: ws.name },
        });
      },
    },
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
    // ── GF-18：单仓直接操作（作用于当前仓库；只编排既有 api，不写业务逻辑）──
    // 网络操作走 GF-07 流式镜像（opId 省略时后端生成），Git Console 自动弹出
    // 展示增量输出与取消入口。
    {
      id: "git:fetch-current",
      title: "Fetch 当前仓库",
      group: "Git 操作",
      run: async () => {
        const repo = requireCurrentRepo();
        const { syncFetch } = await import("@/api/git_ops");
        await syncFetch(repo);
      },
    },
    {
      id: "git:pull-current",
      title: "Pull 当前仓库（Smart Pull）",
      group: "Git 操作",
      run: async () => {
        const repo = requireCurrentRepo();
        const { smartPull } = await import("@/api/git_ops");
        const result = await smartPull(repo);
        // smart_pull 的 Conflict 态会把仓库留在冲突状态——直接打开解决器。
        if (result.status === "conflict") {
          router.push({ name: "conflict-resolver", query: { repo } });
        }
      },
    },
    {
      id: "git:push-current",
      title: "Push 当前仓库",
      group: "Git 操作",
      run: async () => {
        const repo = requireCurrentRepo();
        const { syncPush } = await import("@/api/git_ops");
        await syncPush(repo);
      },
    },
    // ── GF-18：自包含对话框的 prefill 打开（视图保留业务逻辑与确认流）──
    // merge / rebase / cherry-pick / revert / reset 需在视图中选择目标分支/提交，
    // 经既有 git:branch / git:reset 导航到拥有对应 UI 的视图执行。
    {
      id: "git:stash-save",
      title: "Stash 当前仓库（新建记录）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "stash-manager", query: { save: "1" } });
      },
    },
    {
      id: "git:create-pr",
      title: "Create Pull Request（当前仓库）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "branch-manager", query: { pr: "1" } });
      },
    },
    {
      id: "git:worktree-create",
      title: "新建 Worktree（当前仓库）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "worktree-manager", query: { create: "1" } });
      },
    },
    {
      id: "git:rebase",
      title: "Rebase 当前分支（Interactive Rebase）",
      group: "Git 操作",
      run: () => {
        router.push({ name: "branch-manager", query: { rebase: "1" } });
      },
    },
  ];
}

/** 内嵌终端面板命令（TM-02 / TM-07）。 */
function getEmbeddedTerminalCommands(_ctx: CommandContext): Command[] {
  return [
    {
      id: "terminal:toggle",
      title: "切换终端面板",
      group: "终端",
      run: async () => {
        const { useTerminalStore } = await import("@/stores/terminal");
        useTerminalStore().togglePanel();
      },
    },
    {
      id: "terminal:new-shell",
      title: "新建终端 Shell",
      group: "终端",
      run: async () => {
        const { useTerminalStore } = await import("@/stores/terminal");
        const store = useTerminalStore();
        // autoOpen: false —— 本命令自己会 openSession，避免一次开出两个 shell（F-42）
        store.showPanel({ autoOpen: false });
        await store.openSession();
      },
    },
    {
      id: "terminal:search",
      title: "终端内搜索",
      group: "终端",
      run: async () => {
        // 搜索条的开关由 TerminalPanel 内部处理，这里触发面板显示
        const { useTerminalStore } = await import("@/stores/terminal");
        useTerminalStore().showPanel();
        // 通过自定义事件通知 TerminalPanel 打开搜索条
        window.dispatchEvent(new CustomEvent("terminal:toggle-search"));
      },
    },
    // TM-07：复制粘贴（仅终端面板聚焦时生效，不劫持全局）
    {
      id: "terminal:copy",
      title: "终端复制",
      group: "终端",
      run: () => {
        // xterm 的选中文本复制到剪贴板
        const selection = window.getSelection()?.toString();
        if (selection) {
          navigator.clipboard.writeText(selection);
        }
      },
    },
    {
      id: "terminal:paste",
      title: "终端粘贴",
      group: "终端",
      run: async () => {
        // 从剪贴板粘贴到终端（通过 store 写入当前活跃会话）
        const text = await navigator.clipboard.readText();
        if (text) {
          const { useTerminalStore } = await import("@/stores/terminal");
          const store = useTerminalStore();
          const sessionId = store.activeTabId;
          if (sessionId) {
            // 将文本转为 base64（分块编码避免大输入栈溢出）
            await store.writeToSession(sessionId, encodeUtf8Base64(text));
          }
        }
      },
    },
  ];
}

/** 外部终端命令（平台专属类型仅 Windows 列出；不可用时报可行动错误）。 */
function getExternalTerminalCommands(ctx: CommandContext): Command[] {
  const kindTitles: Array<[TerminalKind, string]> =
    window.navigator.platform.toLowerCase().includes("win")
      ? [
          ["system", "在默认终端打开当前仓库"],
          ["powershell", "在 PowerShell 打开当前仓库"],
          ["cmd", "在 CMD 打开当前仓库"],
          ["git-bash", "在 Git Bash 打开当前仓库"],
          ["windows-terminal", "在 Windows Terminal 打开当前仓库"],
        ]
      : [["system", "在外部终端打开当前仓库"]];

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

/** IDE 命令（VS Code / IntelliJ IDEA / Cursor / Zed / Qoder / Qoder CN / CodeBuddy）。 */
function getIdeCommands(ctx: CommandContext): Command[] {
  const ides = Object.keys(IDE_DISPLAY_NAMES) as IdeKind[];

  return ides.map((ide) => ({
    id: `ide:${ide}`,
    title: `在 ${IDE_DISPLAY_NAMES[ide]} 打开当前仓库`,
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

/** 获取所有命令（导航 + 操作 + Git 操作 + 内嵌终端 + 外部终端 + IDE） */
export function getAllCommands(ctx: CommandContext): Command[] {
  return [
    ...getNavigationCommands(ctx),
    ...getActionCommands(ctx),
    ...getGitCommands(ctx),
    ...getEmbeddedTerminalCommands(ctx),
    ...getExternalTerminalCommands(ctx),
    ...getIdeCommands(ctx),
  ];
}
