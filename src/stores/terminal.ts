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
  ShellInfo,
} from "@/api/terminal";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** 前端会话状态（扩展 TerminalSessionInfo，增加 xterm 写缓冲）。 */
export interface TerminalSession extends TerminalSessionInfo {
  /** xterm 实例写缓冲（合并 chunk 后一次性 write）。 */
  writeBuffer: Uint8Array[];
  /** 是否暂停渲染（tab 隐藏时）。 */
  paused: boolean;
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

  /** 事件监听 unlisten 句柄（面板首次打开时注册）。 */
  let unlistenOutput: UnlistenFn | null = null;
  let unlistenExit: UnlistenFn | null = null;
  let listenersRegistered = false;

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
  function registerEventListeners() {
    if (listenersRegistered) return;
    listenersRegistered = true;

    listen<TerminalOutputEvent>(terminalApi.TERMINAL_EVENTS.OUTPUT, (event) => {
      handleOutput(event.payload);
    }).then((unlisten) => {
      unlistenOutput = unlisten;
    });

    listen<TerminalExitEvent>(terminalApi.TERMINAL_EVENTS.EXIT, (event) => {
      handleExit(event.payload);
    }).then((unlisten) => {
      unlistenExit = unlisten;
    });
  }

  /** 处理 terminal_output 事件：合并到写缓冲。 */
  function handleOutput(event: TerminalOutputEvent) {
    const session = sessions.value.find(
      (s) => s.sessionId === event.sessionId
    );
    if (!session) return;

    // base64 解码为 Uint8Array
    const binary = atob(event.dataBase64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }

    if (session.paused) {
      // tab 隐藏时保留数据，切回时补写
      session.writeBuffer.push(bytes);
    } else {
      // 直接写入 xterm（通过事件或直接引用）
      session.writeBuffer.push(bytes);
      // 触发 xterm 刷新（XtermView 组件监听此变化）
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

  /** 打开新 PTY 会话。 */
  async function openSession(params?: {
    cwd?: string;
    shell?: string;
  }): Promise<string> {
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
    }
  }

  /** 关闭指定 tab。 */
  async function closeTab(sessionId: string) {
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
      // 合并：保留现有 session 的 writeBuffer/paused 状态
      const existing = new Map(sessions.value.map((s) => [s.sessionId, s]));
      sessions.value = list.map((info) => {
        const prev = existing.get(info.sessionId);
        return {
          ...info,
          writeBuffer: prev?.writeBuffer ?? [],
          paused: prev?.paused ?? false,
        };
      });
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

  /** 应用退出时清理（teardown 钩子）。 */
  function cleanup() {
    unlistenOutput?.();
    unlistenExit?.();
    unlistenOutput = null;
    unlistenExit = null;
    listenersRegistered = false;
  }

  return {
    // State
    sessions,
    activeTabId,
    panelVisible,
    availableShells,
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
    cleanup,
  };
});
