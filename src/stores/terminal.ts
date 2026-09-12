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
  ShellInfo,
} from "@/api/terminal";
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
  /** TM-06：是否为「在终端中启动」模式（显示降级提示条）。 */
  launchedInTerminal?: boolean;
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
  let unlistenGitOp: UnlistenFn | null = null;
  let unlistenRuntimeOutput: UnlistenFn | null = null;
  let unlistenRuntimeStarted: UnlistenFn | null = null;
  let unlistenRuntimeStopped: UnlistenFn | null = null;
  let listenersRegistered = false;
  let listenersReady: Promise<void> | null = null;

  /** Git Console 会话 ID（固定值，不可关闭）。 */
  const GIT_CONSOLE_SESSION_ID = "__git_console__";

  /** Runtime 会话 ID 前缀（自动创建，可关闭）。 */
  const RUNTIME_SESSION_PREFIX = "__runtime_";

  /** 活跃 runtime 进程列表（用于工具条按钮状态）。 */
  const runtimeProcesses = ref<RuntimeProcessInfo[]>([]);

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
    if (panelVisible.value && !listenersRegistered) {
      registerEventListeners();
    }
  }

  function showPanel() {
    panelVisible.value = true;
    if (!listenersRegistered) {
      registerEventListeners();
    }
  }

  function hidePanel() {
    panelVisible.value = false;
  }

  /** 注册 Tauri 事件监听（面板首次打开时调用，App 生命周期内保持）。 */
  function registerEventListeners(): Promise<void> {
    if (listenersReady) return listenersReady;
    listenersRegistered = true;

    ensureGitConsoleSession();

    listenersReady = (async () => {
      unlistenOutput = await listen<TerminalOutputEvent>(
        terminalApi.TERMINAL_EVENTS.OUTPUT,
        (event) => { handleOutput(event.payload); },
      );

      unlistenExit = await listen<TerminalExitEvent>(
        terminalApi.TERMINAL_EVENTS.EXIT,
        (event) => { handleExit(event.payload); },
      );

      unlistenGitOp = await listen<GitOpOutputEvent>(
        terminalApi.TERMINAL_EVENTS.GIT_OP_OUTPUT,
        (event) => { handleGitOpOutput(event.payload); },
      );

      unlistenRuntimeOutput = await listen<ProcessOutputPayload>(
        RUNTIME_EVENTS.processOutput,
        (event) => { handleRuntimeOutput(event.payload); },
      );

      unlistenRuntimeStarted = await listen(RUNTIME_EVENTS.processStarted, () => {
        refreshRuntimeProcesses();
      });

      unlistenRuntimeStopped = await listen(RUNTIME_EVENTS.processStopped, () => {
        refreshRuntimeProcesses();
      });
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

  /** 确保 Git Console 会话存在（不可关闭的特殊会话）。 */
  function ensureGitConsoleSession() {
    if (sessions.value.some((s) => s.sessionId === GIT_CONSOLE_SESSION_ID)) return;
    sessions.value.unshift({
      sessionId: GIT_CONSOLE_SESSION_ID,
      kind: "shell",
      title: "Git Console",
      cwd: "",
      alive: true,
      writeBuffer: [],
      paused: false,
    });
  }

  /** 处理 terminal_output 事件：通过回调直接写入 xterm，或缓冲到 writeBuffer。 */
  function handleOutput(event: TerminalOutputEvent) {
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
      pendingOutput.get(event.sessionId)!.push(bytes);
      return;
    }

    if (session.paused || !session.writeCallback) {
      // tab 隐藏或 xterm 未挂载时保留数据，切回时补写
      session.writeBuffer.push(bytes);
    } else {
      // 直接写入 xterm（通过注册的回调）
      session.writeCallback(bytes);
    }
  }

  /** 处理 terminal_exit 事件：标记会话死亡。 */
  function handleExit(event: TerminalExitEvent) {
    const session = sessions.value.find(
      (s) => s.sessionId === event.sessionId
    );
    if (session) {
      session.alive = false;
    }
  }

  /** TM-04：处理 git_op_output 事件，写入 Git Console xterm。 */
  function handleGitOpOutput(event: GitOpOutputEvent) {
    const session = sessions.value.find(
      (s) => s.sessionId === GIT_CONSOLE_SESSION_ID
    );
    if (!session) return;

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

  /** TM-06：在终端中启动 runtime。 */
  async function launchInTerminal(command: string, cwd?: string, env?: Record<string, string>) {
    showPanel();
    if (listenersReady) await listenersReady;
    const sessionId = await terminalApi.runtimeStartInTerminal(command, cwd, env);
    const session: TerminalSession = {
      sessionId,
      kind: "shell",
      title: "Terminal",
      cwd: cwd ?? "",
      alive: true,
      writeBuffer: [],
      paused: false,
      launchedInTerminal: true,
    };

    // 排干 await 期间缓冲的 PTY 输出
    const pending = pendingOutput.get(sessionId);
    if (pending) {
      session.writeBuffer.push(...pending);
      pendingOutput.delete(sessionId);
    }

    sessions.value.push(session);
    activeTabId.value = sessionId;
  }

  /** 打开新 PTY 会话。 */
  async function openSession(params?: {
    cwd?: string;
    shell?: string;
  }): Promise<string> {
    // 确保事件监听器已注册（PTY reader 在 IPC 返回前就会开始发送事件）
    if (listenersReady) await listenersReady;
    const sessionId = await terminalApi.terminalOpen({
      cwd: params?.cwd,
      shell: params?.shell,
      cols: 80,
      rows: 24,
    });

    const session: TerminalSession = {
      sessionId,
      kind: "shell",
      title: params?.shell ?? "Shell",
      cwd: params?.cwd ?? "",
      alive: true,
      writeBuffer: [],
      paused: false,
    };

    // 排干 await 期间缓冲的 PTY 输出（shell prompt 等）
    const pending = pendingOutput.get(sessionId);
    if (pending) {
      session.writeBuffer.push(...pending);
      pendingOutput.delete(sessionId);
    }

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
    // Git Console 不可关闭
    if (sessionId === GIT_CONSOLE_SESSION_ID) return;
    pendingOutput.delete(sessionId);

    try {
      await terminalApi.terminalClose({ sessionId });
    } catch (e) {
      console.warn("terminal close failed:", e);
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
          writeBuffer: prev?.writeBuffer ?? [],
          paused: prev?.paused ?? false,
          writeCallback: prev?.writeCallback,
          launchedInTerminal: prev?.launchedInTerminal,
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
    unlistenGitOp?.();
    unlistenRuntimeOutput?.();
    unlistenRuntimeStarted?.();
    unlistenRuntimeStopped?.();
    unlistenOutput = null;
    unlistenExit = null;
    unlistenGitOp = null;
    unlistenRuntimeOutput = null;
    unlistenRuntimeStarted = null;
    unlistenRuntimeStopped = null;
    listenersRegistered = false;
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
    launchInTerminal,
    refreshRuntimeProcesses,
    cleanup,
  };
});
