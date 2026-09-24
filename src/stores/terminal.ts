/**
 * Terminal store — 会话状态管理（TM-02，terminal-feature-plan §4.3）。
 *
 * 会话列表（kind/title/cwd/alive）、activeTab、panelVisible、每会话写缓冲。
 * 事件监听在面板首次打开时注册，App 生命周期内保持。
 */

import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as terminalApi from "@/api/terminal";
import type {
  TerminalSessionInfo,
  TerminalOutputEvent,
  TerminalExitEvent,
  GitOpOutputEvent,
  GitOpStartedEvent,
  GitOpFinishedEvent,
  ShellInfo,
} from "@/api/terminal";
import * as gitOpsApi from "@/api/git_ops";
import { classifyGitErrorText, type GitErrorGuidance } from "@/utils/gitError";
import { RUNTIME_EVENTS } from "@/api/runtime";
import type { ProcessOutputPayload, RuntimeProcessInfo } from "@/types/runtime";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** 前端会话状态（扩展 TerminalSessionInfo，增加 xterm 写缓冲）。 */
export interface TerminalSession extends TerminalSessionInfo {
  /** xterm 实例写缓冲（合并 chunk 后一次性 write）。 */
  writeBuffer: Uint8Array[];
  /** 是否暂停渲染（tab 隐藏时）。 */
  paused: boolean;
  /** xterm 写入回调（XtermView 挂载时注册，卸载时清除）。 */
  writeCallback?: (data: Uint8Array) => void;
}

/**
 * PAF-12：writeBuffer / pendingOutput 的块数上限（对照 runtime logBuffers
 * 5000 行环形上限）。面板隐藏（v-if 卸载 XtermView）或 xterm 未挂载期间
 * 输出全部入缓冲，超限丢最旧，防止长跑 runtime 输出把内存打爆。
 */
const WRITE_BUFFER_MAX_CHUNKS = 5000;

/** 超限丢最旧（splice O(n) 一次裁剪，优于 shift 循环 O(n*m)）。 */
function trimWriteBuffer(buffer: Uint8Array[]) {
  if (buffer.length > WRITE_BUFFER_MAX_CHUNKS) {
    buffer.splice(0, buffer.length - WRITE_BUFFER_MAX_CHUNKS);
  }
}

/** Git Console 会话 ID（前端独有镜像 tab，Rust 侧不存在此会话）。 */
const GIT_CONSOLE_SESSION_ID = "__git_console__";

/** Runtime 输出镜像会话 ID 前缀（自动创建，可关闭）。 */
const RUNTIME_SESSION_PREFIX = "__runtime_";

/** 装依赖输出镜像会话 ID 前缀（TM-08）。 */
const INSTALL_SESSION_PREFIX = "__install_";

/** 构建输出镜像会话 ID 前缀（TM-08）。 */
const BUILD_SESSION_PREFIX = "__build_";

/**
 * GF-07：进行中的单仓网络操作（Git Console 取消入口的数据源）。
 * `git_op_started` 入列、`git_op_finished` 出列；事件监听在 App 级
 * （useGitOpMirror，对齐 useTaskProgress），面板懒注册（F-42）不会丢。
 */
export interface GitOpInFlight {
  opId: string;
  repoPath: string;
  repoName: string;
  command: string;
}

/** GF-08：最近一次失败的单仓网络操作 + 分类后的可行动引导。 */
export interface GitOpFailure {
  repoName: string;
  command: string;
  error: string;
  guidance: GitErrorGuidance | null;
}

/** 判定「真正的终端会话」：排除 Git Console 与全部输出镜像 tab。 */
function isRealShellSession(session: TerminalSession): boolean {
  return (
    session.sessionId !== GIT_CONSOLE_SESSION_ID &&
    !session.sessionId.startsWith(RUNTIME_SESSION_PREFIX) &&
    !session.sessionId.startsWith(INSTALL_SESSION_PREFIX) &&
    !session.sessionId.startsWith(BUILD_SESSION_PREFIX)
  );
}

/** `node_install_output` 事件 payload（task worker 发射）。 */
interface NodeInstallOutputPayload {
  taskId: string;
  stream: "stdout" | "stderr";
  line: string;
}

/** `runtime_build_output` 事件 payload（EmittingBuildSink 发射）。 */
interface BuildOutputEventPayload {
  runtimeName: string;
  stream: "stdout" | "stderr";
  line: string;
}

/** 排干 await IPC 期间缓冲的 PTY 输出（shell prompt 等）并入会话。 */
function drainPendingOutput(
  pendingOutput: Map<string, Uint8Array[]>,
  sessionId: string,
  session: TerminalSession,
) {
  const pending = pendingOutput.get(sessionId);
  if (!pending || pending.length === 0) return;
  // 用 concat 而非展开：首屏可能是大块输出，spread 有爆栈风险（同 PAF-04）
  session.writeBuffer = session.writeBuffer.concat(pending);
  trimWriteBuffer(session.writeBuffer);
  pendingOutput.delete(sessionId);
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const useTerminalStore = defineStore("terminal", () => {
  // -- State --
  const sessions = ref<TerminalSession[]>([]);
  const activeTabId = ref<string | null>(null);
  const panelVisible = ref(false);
  const availableShells = ref<ShellInfo[]>([]);

  /** PTY 输出缓冲：session 尚未 push 到 sessions.value 期间（await IPC 返回前）的输出。 */
  const pendingOutput = new Map<string, Uint8Array[]>();

  /** 事件监听 unlisten 句柄（面板首次打开时注册）。 */
  let unlistenOutput: UnlistenFn | null = null;
  let unlistenExit: UnlistenFn | null = null;
  let unlistenRuntimeOutput: UnlistenFn | null = null;
  let unlistenRuntimeStarted: UnlistenFn | null = null;
  let unlistenRuntimeStopped: UnlistenFn | null = null;
  let unlistenNodeInstallOutput: UnlistenFn | null = null;
  let unlistenBuildOutput: UnlistenFn | null = null;
  let listenersReady: Promise<void> | null = null;

  /** 活跃 runtime 进程列表（用于工具条按钮状态）。 */
  const runtimeProcesses = ref<RuntimeProcessInfo[]>([]);

  /** GF-07：进行中的单仓网络操作（Git Console 取消入口数据源）。 */
  const gitOpsInFlight = ref<GitOpInFlight[]>([]);

  /**
   * GF-08：最近一次失败的单仓网络操作（Git Console 失败引导数据源）。
   * `git_op_finished(success=false)` 写入、新 `git_op_started` 清除；
   * 携带分类后的可行动引导（reason / actions），面板据此渲染动作入口。
   */
  const lastGitOpFailure = ref<GitOpFailure | null>(null);

  /**
   * TM-08：node_install 任务 id → 应用名映射。装依赖经 N-08 两跳确认流程，
   * 任务 id 与 runtime 名的关联只在前端（提交任务处）可得，故由调用方在提交
   * 后回调 `bindInstallTask` 登记。
   */
  const installTaskNames = new Map<string, string>();

  // -- Getters --
  const activeSession = computed(() =>
    sessions.value.find((s) => s.sessionId === activeTabId.value) ?? null
  );

  const aliveSessions = computed(() =>
    sessions.value.filter((s) => s.alive)
  );

  // -- Actions --

  /** 切换面板开合。 */
  function togglePanel() {
    panelVisible.value = !panelVisible.value;
    if (panelVisible.value) {
      openPanel(true);
    }
  }

  /**
   * 显示面板。
   *
   * `autoOpen` 默认 true：打开面板时保证有一个可用的真终端（见 `ensureRealSession`）。
   * 调用方**自身会立刻创建会话**时传 false，避免一次操作开出两个 shell
   * （`terminal:new-shell` 命令、`focusMirrorTab`）。
   */
  function showPanel(options?: { autoOpen?: boolean }) {
    panelVisible.value = true;
    openPanel(options?.autoOpen ?? true);
  }

  function hidePanel() {
    panelVisible.value = false;
  }

  /** 面板打开后的统一初始化：注册事件监听（一次性）→ 保证存在一个真终端。 */
  function openPanel(autoOpen: boolean) {
    void (async () => {
      try {
        await registerEventListeners();
      } catch (e) {
        // 事件通道全挂时开出的会话收不到任何输出，放弃自动开（与旧行为一致）
        console.warn("terminal: event listener registration failed (panel open):", e);
        return;
      }
      if (autoOpen) await ensureRealSession();
    })();
  }

  /**
   * 保证面板里有一个可用的真终端（幂等，重复调用不重复建会话）。
   *
   * - 已有**存活**的真终端：只在当前 tab 失效时接管焦点（无选中 / 会话已退出 /
   *   Git Console 镜像 tab）——不抢走用户自己选中的真终端或 runtime 输出 tab；
   * - 没有任何存活真终端：新建一个，默认 shell 由后端 §5.1 顺序决定
   *   （Windows: PowerShell 7 → Windows PowerShell → CMD）。
   */
  async function ensureRealSession() {
    const live = sessions.value.find((s) => isRealShellSession(s) && s.alive);
    if (live) {
      const current = sessions.value.find((s) => s.sessionId === activeTabId.value);
      if (!current || !current.alive || current.sessionId === GIT_CONSOLE_SESSION_ID) {
        switchTab(live.sessionId);
      }
      return;
    }
    await openSession();
  }

  /** 注册 Tauri 事件监听（面板首次打开时调用，App 生命周期内保持）。
   *  PAF-16：逐个独立注册，单个 listen 失败降级为「该事件不可用」（记错误
   *  日志，成功者保留）；若全部失败则回滚注册门槛，下次打开面板可重试。
   *  评审优化：Promise.allSettled 并行注册，避免单个 listen 卡住阻塞其余。 */
  function registerEventListeners(): Promise<void> {
    if (listenersReady) return listenersReady;

    const listenerDefs = [
      {
        event: terminalApi.TERMINAL_EVENTS.OUTPUT,
        handler: (e: { payload: TerminalOutputEvent }) => handleOutput(e.payload),
        assign: (un: UnlistenFn) => { unlistenOutput = un; },
      },
      {
        event: terminalApi.TERMINAL_EVENTS.EXIT,
        handler: (e: { payload: TerminalExitEvent }) => handleExit(e.payload),
        assign: (un: UnlistenFn) => { unlistenExit = un; },
      },
      {
        event: RUNTIME_EVENTS.processOutput,
        handler: (e: { payload: ProcessOutputPayload }) => handleRuntimeOutput(e.payload),
        assign: (un: UnlistenFn) => { unlistenRuntimeOutput = un; },
      },
      {
        event: RUNTIME_EVENTS.processStarted,
        handler: () => { refreshRuntimeProcesses(); },
        assign: (un: UnlistenFn) => { unlistenRuntimeStarted = un; },
      },
      {
        event: RUNTIME_EVENTS.processStopped,
        handler: () => { refreshRuntimeProcesses(); },
        assign: (un: UnlistenFn) => { unlistenRuntimeStopped = un; },
      },
      {
        // TM-08：装依赖逐行输出（后端 task worker 已发射；前端此前无消费方）。
        event: "node_install_output",
        handler: (e: { payload: NodeInstallOutputPayload }) =>
          handleNodeInstallOutput(e.payload),
        assign: (un: UnlistenFn) => { unlistenNodeInstallOutput = un; },
      },
      {
        // TM-08：构建逐行输出（EmittingBuildSink 经 emitter 发射）。
        event: RUNTIME_EVENTS.buildOutput,
        handler: (e: { payload: BuildOutputEventPayload }) =>
          handleBuildOutput(e.payload),
        assign: (un: UnlistenFn) => { unlistenBuildOutput = un; },
      },
    ];

    listenersReady = (async () => {
      const results = await Promise.allSettled(
        listenerDefs.map((def) =>
          listen(def.event, def.handler as (event: unknown) => void).then((un) => {
            def.assign(un);
            return un;
          }),
        ),
      );

      const succeeded = results.filter((r): r is PromiseFulfilledResult<UnlistenFn> =>
        r.status === "fulfilled",
      );
      const failed = results.filter((r): r is PromiseRejectedResult =>
        r.status === "rejected",
      );

      for (const r of failed) {
        console.error("terminal: register event listener failed:", r.reason);
      }

      if (succeeded.length === 0) {
        // 回滚注册门槛，下次打开面板可重试
        listenersReady = null;
        throw new Error("terminal: all event listeners failed to register");
      }
    })();

    return listenersReady;
  }

  /** 刷新 runtime 进程列表（用于工具条按钮状态）。 */
  async function refreshRuntimeProcesses() {
    try {
      const { runtimeListProcesses } = await import("@/api/runtime");
      // 需要 workspaceId，从 workspace store 获取
      const { useWorkspaceStore } = await import("@/stores/workspace");
      const wsId = useWorkspaceStore().currentWorkspace?.id;
      if (wsId) {
        runtimeProcesses.value = await runtimeListProcesses(wsId);
      }
    } catch (e) {
      console.error("Failed to refresh runtime processes:", e);
    }
  }

  /**
   * 确保 Git Console 会话存在并返回它。
   *
   * **懒创建**（F-42）：不再在打开面板时无条件挂一个 tab，只在真正有
   * `git_op_output` 镜像输出时才出现——它是个输出镜像，不是真终端，
   * 常驻会让面板默认落在关不掉的冗余 tab 上。
   */
  function ensureGitConsoleSession(): TerminalSession {
    const existing = sessions.value.find(
      (s) => s.sessionId === GIT_CONSOLE_SESSION_ID
    );
    if (existing) return existing;

    const session: TerminalSession = {
      sessionId: GIT_CONSOLE_SESSION_ID,
      kind: "shell",
      title: "Git Console",
      cwd: "",
      alive: true,
      writeBuffer: [],
      paused: false,
    };
    // 追加到末尾：懒创建时不打乱已有 tab 顺序
    sessions.value.push(session);
    return session;
  }

  /**
   * TM-08：确保指定 id 的输出镜像 tab 存在并返回它（幂等）。
   *
   * 与 ensureGitConsoleSession 同一套懒创建模式；kind 恒为 "runtime"
   * （TerminalPanel 据此隐藏交互输入，见 isRuntimeTab）。
   */
  function ensureMirrorSession(sessionId: string, title: string): TerminalSession {
    const existing = sessions.value.find((s) => s.sessionId === sessionId);
    if (existing) return existing;
    const session: TerminalSession = {
      sessionId,
      kind: "runtime",
      title,
      cwd: "",
      alive: true,
      writeBuffer: [],
      paused: false,
    };
    sessions.value.push(session);
    return session;
  }

  /** 向输出镜像 tab 写一行（stderr 黄色着色；`\r\n` 收尾适配 xterm）。 */
  function writeMirrorLine(sessionId: string, line: string, stream: string) {
    const session = ensureMirrorSession(sessionId, sessionId);
    const colored = stream === "stderr" ? `\x1b[33m${line}\x1b[0m` : line;
    const bytes = new TextEncoder().encode(`${colored}\r\n`);
    if (session.writeCallback) {
      session.writeCallback(bytes);
    } else {
      session.writeBuffer.push(bytes);
      trimWriteBuffer(session.writeBuffer);
    }
  }

  /** TM-08：node_install_output —— 写入 `__install_<应用名>` 镜像 tab。 */
  function handleNodeInstallOutput(payload: NodeInstallOutputPayload) {
    const runtimeName = installTaskNames.get(payload.taskId);
    // 未登记映射（非 RuntimeDashboard 装依赖流程发起的 install）不镜像，
    // 避免出现无主 tab。
    if (!runtimeName) return;
    writeMirrorLine(`${INSTALL_SESSION_PREFIX}${runtimeName}`, payload.line, payload.stream);
  }

  /** TM-08：runtime_build_output —— 写入 `__build_<应用名>` 镜像 tab。 */
  function handleBuildOutput(payload: BuildOutputEventPayload) {
    writeMirrorLine(
      `${BUILD_SESSION_PREFIX}${payload.runtimeName}`,
      payload.line,
      payload.stream
    );
  }

  /** TM-08：登记 node_install 任务 id → 应用名（提交任务后调用）。 */
  function bindInstallTask(taskId: string, runtimeName: string) {
    installTaskNames.set(taskId, runtimeName);
  }

  /**
   * TM-08：打开面板并聚焦输出镜像 tab（不自动开真终端）。
   *
   * tab 不存在时预创建——「点了启动/构建就在终端面板里看这个应用」的
   * 操作习惯要求启动瞬间即可聚焦。同一应用重复触发聚焦已有 tab。
   */
  function focusMirrorTab(sessionId: string, title: string) {
    ensureMirrorSession(sessionId, title);
    showPanel({ autoOpen: false });
    switchTab(sessionId);
  }

  /** 处理 terminal_output 事件：通过回调直接写入 xterm，或缓冲到 writeBuffer。 */  function handleOutput(event: TerminalOutputEvent) {
    // base64 解码为 Uint8Array
    const binary = atob(event.dataBase64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }

    const session = sessions.value.find(
      (s) => s.sessionId === event.sessionId
    );
    if (!session) {
      // session 尚未 push（await IPC 返回前 PTY reader 已开始发送），缓冲
      if (!pendingOutput.has(event.sessionId)) {
        pendingOutput.set(event.sessionId, []);
      }
      const pending = pendingOutput.get(event.sessionId)!;
      pending.push(bytes);
      trimWriteBuffer(pending);
      return;
    }

    if (session.paused || !session.writeCallback) {
      // tab 隐藏或 xterm 未挂载时保留数据，切回时补写（PAF-12：超限丢最旧）
      session.writeBuffer.push(bytes);
      trimWriteBuffer(session.writeBuffer);
    } else {
      // 直接写入 xterm（通过注册的回调）
      session.writeCallback(bytes);
    }
  }

  /** 处理 terminal_exit 事件：标记会话死亡。 */
  async function handleExit(event: TerminalExitEvent) {
    const session = sessions.value.find(
      (s) => s.sessionId === event.sessionId
    );
    if (session) {
      session.alive = false;
    }
  }

  /**
   * TM-04：git_op_output —— 写入 Git Console xterm（懒创建，见 F-42）。
   * 事件监听在 App 级（useGitOpMirror），面板未开时输出进 tab 缓冲，
   * 打开面板后由 registerWriteCallback 补写，不丢。
   */
  function onGitOpOutput(event: GitOpOutputEvent) {
    const session = ensureGitConsoleSession();

    // 格式化输出行
    let line = event.line;
    if (event.stream === "meta") {
      // meta 行（$ git commit -m "..." 样式）加前缀标识
      line = `\x1b[36m${line}\x1b[0m`; // cyan 色
    } else if (event.stream === "stderr") {
      line = `\x1b[33m${line}\x1b[0m`; // yellow 色
    }

    // 追加仓库名标识（批量操作时可辨识）
    const prefix = event.repoName ? `[${event.repoName}] ` : "";
    const fullLine = `${prefix}${line}\r\n`;

    // 写入 xterm
    const encoder = new TextEncoder();
    const bytes = encoder.encode(fullLine);

    if (session.writeCallback) {
      session.writeCallback(bytes);
    } else {
      session.writeBuffer.push(bytes);
      trimWriteBuffer(session.writeBuffer);
    }
  }

  /**
   * GF-07：git_op_started —— 单仓网络操作开始。登记取消入口数据，
   * 并弹出终端面板聚焦 Git Console（TM-08 focusMirrorTab 同款模式：
   * 「点了操作就在终端面板里看输出」，不再静默转圈）。
   */
  function onGitOpStarted(event: GitOpStartedEvent) {
    if (!gitOpsInFlight.value.some((op) => op.opId === event.opId)) {
      gitOpsInFlight.value.push({
        opId: event.opId,
        repoPath: event.repoPath,
        repoName: event.repoName,
        command: event.command,
      });
    }
    // GF-08：新操作开始即清理上一次失败的引导。
    lastGitOpFailure.value = null;
    ensureGitConsoleSession();
    showPanel({ autoOpen: false });
    switchTab(GIT_CONSOLE_SESSION_ID);
  }

  /** GF-07：git_op_finished —— 出列取消入口（幂等）。 */
  function onGitOpFinished(event: GitOpFinishedEvent) {
    // 先取快照再出列（出列后找不到 op 的 repo/command）。
    const finished = gitOpsInFlight.value.find((op) => op.opId === event.opId);
    gitOpsInFlight.value = gitOpsInFlight.value.filter((op) => op.opId !== event.opId);
    if (!event.success && event.error) {
      // GF-08：记录失败 + 分类（纯文本镜像后端 classify_git_error——
      // git_op_finished 只带 error 字符串，details 不可达）。
      lastGitOpFailure.value = {
        repoName: finished?.repoName ?? "",
        command: finished?.command ?? "",
        error: event.error,
        guidance: classifyGitErrorText(event.error),
      };
    }
  }

  /**
   * GF-07：取消一个单仓网络操作。后端可能已自然结束（NotFound）——
   * 本地同样出列，让取消入口立即消失。
   */
  async function cancelGitOp(opId: string) {
    try {
      await gitOpsApi.cancelGitOp(opId);
    } catch (e) {
      console.warn("cancel git op failed (likely already finished):", e);
    } finally {
      gitOpsInFlight.value = gitOpsInFlight.value.filter((op) => op.opId !== opId);
    }
  }

  /** 每个 runtime tab 的当前 phase（用于检测 phase 切换）。 */
  const runtimePhaseMap = new Map<string, string>();

  /** TM-05：处理 runtime_process_output 事件，写入对应 runtime tab xterm。 */
  function handleRuntimeOutput(event: ProcessOutputPayload) {
    const sessionId = `${RUNTIME_SESSION_PREFIX}${event.runtimeName}`;
    let session = sessions.value.find((s) => s.sessionId === sessionId);

    // 自动创建 runtime tab（如果不存在）
    if (!session) {
      session = {
        sessionId,
        kind: "runtime",
        title: event.runtimeName,
        cwd: "",
        alive: true,
        writeBuffer: [],
        paused: false,
      };
      sessions.value.push(session);
      // 补写缓冲已有内容（从 runtime store 的 logBuffers）
      loadExistingRuntimeLogs(event.runtimeName, session);
    }

    // 格式化 LogLine 为 ANSI 字符串
    const encoder = new TextEncoder();
    for (const logLine of event.lines) {
      // TM-05：phase 分隔行（build → run 切换时）
      const currentPhase = runtimePhaseMap.get(event.runtimeName);
      if (logLine.phase && logLine.phase !== currentPhase) {
        runtimePhaseMap.set(event.runtimeName, logLine.phase);
        const separator = `\r\n\x1b[36m── ${logLine.phase} ──\x1b[0m\r\n\r\n`;
        const sepBytes = encoder.encode(separator);
        if (session.writeCallback) {
          session.writeCallback(sepBytes);
        } else {
          session.writeBuffer.push(sepBytes);
          trimWriteBuffer(session.writeBuffer);
        }
      }

      let line = logLine.line;
      // stderr 行着色区分
      if (logLine.stream === "stderr") {
        line = `\x1b[33m${line}\x1b[0m`; // yellow
      }
      const fullLine = `${line}\r\n`;
      const bytes = encoder.encode(fullLine);

      if (session.writeCallback) {
        session.writeCallback(bytes);
      } else {
        session.writeBuffer.push(bytes);
        trimWriteBuffer(session.writeBuffer);
      }
    }
  }

  /** 补写 runtime store 的 logBuffers 已有内容到新创建的 runtime tab。 */
  async function loadExistingRuntimeLogs(runtimeName: string, session: TerminalSession) {
    try {
      const { useRuntimeStore } = await import("@/stores/runtime");
      const runtimeStore = useRuntimeStore();
      const logBuffer = runtimeStore.logBuffers.get(runtimeName);
      if (!logBuffer || logBuffer.length === 0) return;

      const encoder = new TextEncoder();
      for (const logLine of logBuffer) {
        // phase 分隔
        const currentPhase = runtimePhaseMap.get(runtimeName);
        if (logLine.phase && logLine.phase !== currentPhase) {
          runtimePhaseMap.set(runtimeName, logLine.phase);
          const separator = `\r\n\x1b[36m── ${logLine.phase} ──\x1b[0m\r\n\r\n`;
          const sepBytes = encoder.encode(separator);
          if (session.writeCallback) {
            session.writeCallback(sepBytes);
          } else {
            session.writeBuffer.push(sepBytes);
            trimWriteBuffer(session.writeBuffer);
          }
        }

        let line = logLine.line;
        if (logLine.stream === "stderr") {
          line = `\x1b[33m${line}\x1b[0m`;
        }
        const fullLine = `${line}\r\n`;
        const bytes = encoder.encode(fullLine);
        if (session.writeCallback) {
          session.writeCallback(bytes);
        } else {
          session.writeBuffer.push(bytes);
          trimWriteBuffer(session.writeBuffer);
        }
      }
    } catch (e) {
      console.error("Failed to load existing runtime logs:", e);
    }
  }

  /** TM-05：检查 runtime 是否正在运行。 */
  function isRuntimeRunning(runtimeName: string): boolean {
    return runtimeProcesses.value.some(
      (p) => p.runtimeName === runtimeName && p.status === "running"
    );
  }

  /** TM-05：检查 runtime 是否正在构建中。 */
  function isRuntimeBuilding(runtimeName: string): boolean {
    return runtimeProcesses.value.some(
      (p) =>
        p.runtimeName === runtimeName &&
        ["preparing", "resolving", "building", "starting"].includes(p.status)
    );
  }

  /** TM-05：启动 runtime。 */
  async function startRuntime(runtimeName: string) {
    const { useRuntimeStore } = await import("@/stores/runtime");
    await useRuntimeStore().start(runtimeName);
  }

  /** TM-05：停止 runtime。 */
  async function stopRuntime(runtimeName: string) {
    const { useRuntimeStore } = await import("@/stores/runtime");
    await useRuntimeStore().stop(runtimeName);
  }

  /** TM-05：重启 runtime。 */
  async function restartRuntime(runtimeName: string) {
    const { useRuntimeStore } = await import("@/stores/runtime");
    await useRuntimeStore().restart(runtimeName);
  }

  /** 打开新 PTY 会话。 */
  async function openSession(params?: {
    cwd?: string;
    shell?: string;
  }): Promise<string> {
    // 确保事件监听器已注册（PTY reader 在 IPC 返回前就会开始发送事件）
    if (listenersReady) await listenersReady;

    // F-42：不指定 shell 时用探测列表的第一个——后端 §5.1 顺序，
    // Windows 上即 PowerShell 7（无 pwsh 时回退 Windows PowerShell / CMD）。
    // 探测失败则传 undefined，由后端 detect_default_shell() 兜底。
    if (!params?.shell && availableShells.value.length === 0) {
      await loadShells();
    }
    const shellId = params?.shell ?? availableShells.value[0]?.id;
    const title =
      availableShells.value.find((s) => s.id === shellId)?.label ??
      shellId ??
      "Shell";

    const sessionId = await terminalApi.terminalOpen({
      cwd: params?.cwd,
      shell: shellId,
      cols: 80,
      rows: 24,
    });

    // 打开面板时的 refreshSessions 可能已把该会话（Rust 侧已创建）并入列表，
    // 直接复用，避免同一个 sessionId 出现两个 tab。
    const known = sessions.value.find((s) => s.sessionId === sessionId);
    if (known) {
      drainPendingOutput(pendingOutput, sessionId, known);
      activeTabId.value = sessionId;
      return sessionId;
    }

    const session: TerminalSession = {
      sessionId,
      kind: "shell",
      title,
      cwd: params?.cwd ?? "",
      alive: true,
      writeBuffer: [],
      paused: false,
    };

    // 排干 await 期间缓冲的 PTY 输出（shell prompt 等）
    drainPendingOutput(pendingOutput, sessionId, session);

    sessions.value.push(session);
    activeTabId.value = sessionId;

    return sessionId;
  }

  /** 切换到指定 tab。 */
  function switchTab(sessionId: string) {
    const session = sessions.value.find((s) => s.sessionId === sessionId);
    if (session) {
      // 从暂停恢复
      session.paused = false;
      activeTabId.value = sessionId;
      // 补写暂停期间缓冲的数据
      if (session.writeBuffer.length > 0 && session.writeCallback) {
        for (const chunk of session.writeBuffer) {
          session.writeCallback(chunk);
        }
        session.writeBuffer = [];
      }
    }
  }

  /** 关闭指定 tab。 */
  async function closeTab(sessionId: string) {
    pendingOutput.delete(sessionId);

    // Git Console 是前端独有镜像 tab（Rust 侧无此会话），无需走 close；
    // 关掉后下次收到 git_op_output 会重新懒创建（F-42）。
    if (sessionId !== GIT_CONSOLE_SESSION_ID) {
      try {
        await terminalApi.terminalClose({ sessionId });
      } catch (e) {
        console.warn("terminal close failed:", e);
      }
    }

    const idx = sessions.value.findIndex((s) => s.sessionId === sessionId);
    if (idx !== -1) {
      sessions.value.splice(idx, 1);
    }

    // 切换到相邻 tab
    if (activeTabId.value === sessionId) {
      if (sessions.value.length > 0) {
        const newIdx = Math.min(idx, sessions.value.length - 1);
        activeTabId.value = sessions.value[newIdx].sessionId;
      } else {
        activeTabId.value = null;
      }
    }
  }

  /** 刷新会话列表（面板重开时恢复）。 */
  async function refreshSessions() {
    try {
      const list = await terminalApi.terminalList();
      const existing = new Map(sessions.value.map((s) => [s.sessionId, s]));
      const rustIds = new Set(list.map((s) => s.sessionId));
      sessions.value = list.map((info) => {
        const prev = existing.get(info.sessionId);
        return {
          ...info,
          // 保留前端标题：Rust 侧 title 是 exe 文件名（pwsh.exe），
          // 前端标题是 shell label（PowerShell 7），重开面板不该退化（F-42）
          title: prev?.title ?? info.title,
          writeBuffer: prev?.writeBuffer ?? [],
          paused: prev?.paused ?? false,
          writeCallback: prev?.writeCallback,
        };
      });
      // 保留前端独有的 session（Git Console、runtime tab 等，不存在于 Rust HashMap）
      for (const prev of existing.values()) {
        if (!rustIds.has(prev.sessionId)) {
          sessions.value.push(prev);
        }
      }
      // 如果 activeTab 不在列表中，选第一个
      if (activeTabId.value && !sessions.value.some((s) => s.sessionId === activeTabId.value)) {
        activeTabId.value = sessions.value[0]?.sessionId ?? null;
      }
    } catch (e) {
      console.error("terminal list failed:", e);
    }
  }

  /** 加载可用 shell 列表。 */
  async function loadShells() {
    try {
      availableShells.value = await terminalApi.terminalListShells();
    } catch (e) {
      console.error("terminal list_shells failed:", e);
    }
  }

  /** 写入 base64 数据到指定会话。 */
  async function writeToSession(sessionId: string, dataBase64: string) {
    await terminalApi.terminalWrite({ sessionId, dataBase64 });
  }

  /** 缩放指定会话。 */
  async function resizeSession(sessionId: string, cols: number, rows: number) {
    await terminalApi.terminalResize({ sessionId, cols, rows });
  }

  /** 清空会话的写缓冲（xterm 写入后调用）。 */
  function flushBuffer(sessionId: string) {
    const session = sessions.value.find((s) => s.sessionId === sessionId);
    if (session) {
      session.writeBuffer = [];
    }
  }

  /** 标记 tab 隐藏（暂停渲染，保留数据）。 */
  function pauseSession(sessionId: string) {
    const session = sessions.value.find((s) => s.sessionId === sessionId);
    if (session) {
      session.paused = true;
    }
  }

  /** 注册 xterm 写入回调（XtermView 挂载时调用）。 */
  function registerWriteCallback(sessionId: string, callback: (data: Uint8Array) => void) {
    const session = sessions.value.find((s) => s.sessionId === sessionId);
    if (session) {
      session.writeCallback = callback;
      // 补写已缓冲的数据
      if (session.writeBuffer.length > 0) {
        for (const chunk of session.writeBuffer) {
          callback(chunk);
        }
        session.writeBuffer = [];
      }
    }
  }

  /** 注销 xterm 写入回调（XtermView 卸载时调用）。 */
  function unregisterWriteCallback(sessionId: string) {
    const session = sessions.value.find((s) => s.sessionId === sessionId);
    if (session) {
      session.writeCallback = undefined;
    }
  }

  /** 应用退出时清理（teardown 钩子）。 */
  function cleanup() {
    unlistenOutput?.();
    unlistenExit?.();
    unlistenRuntimeOutput?.();
    unlistenRuntimeStarted?.();
    unlistenRuntimeStopped?.();
    unlistenNodeInstallOutput?.();
    unlistenBuildOutput?.();
    unlistenNodeInstallOutput = null;
    unlistenBuildOutput = null;
    unlistenOutput = null;
    unlistenExit = null;
    unlistenRuntimeOutput = null;
    unlistenRuntimeStarted = null;
    unlistenRuntimeStopped = null;
    listenersReady = null;
    pendingOutput.clear();
  }

  return {
    // State
    sessions,
    activeTabId,
    panelVisible,
    availableShells,
    runtimeProcesses,
    // GF-07：进行中的单仓网络操作（Git Console 取消入口）
    gitOpsInFlight,
    // GF-08：最近一次失败的单仓网络操作（Git Console 失败引导）
    lastGitOpFailure,
    // Getters
    activeSession,
    aliveSessions,
    // Actions
    togglePanel,
    showPanel,
    hidePanel,
    openSession,
    switchTab,
    closeTab,
    refreshSessions,
    loadShells,
    writeToSession,
    resizeSession,
    flushBuffer,
    pauseSession,
    registerWriteCallback,
    unregisterWriteCallback,
    isRuntimeRunning,
    isRuntimeBuilding,
    startRuntime,
    stopRuntime,
    restartRuntime,
    focusMirrorTab,
    bindInstallTask,
    refreshRuntimeProcesses,
    // GF-07：git_op_* 事件入口（App 级 useGitOpMirror 调用）
    onGitOpOutput,
    onGitOpStarted,
    onGitOpFinished,
    cancelGitOp,
    cleanup,
  };
});
