import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";
import { useTerminalStore } from "@/stores/terminal";
import {
  GIT_OP_EVENTS,
  type GitOpOutputEvent,
  type GitOpStartedEvent,
  type GitOpFinishedEvent,
} from "@/api/terminal";

/**
 * GF-07：git_op_* 事件监听上提到 App 级（对齐 useTaskProgress）。
 *
 * 单仓网络操作（sync_fetch/pull/push、smart_pull、push_branch）的 Git Console
 * 镜像与「进行中 → 取消入口」状态需要全程在线：终端面板的事件注册是懒的
 * （F-42，首次打开面板才注册），而网络操作可能在面板从未打开时发生——
 * 那批事件会整段丢失，用户又回到「静默转圈」。
 */
export function useGitOpMirror() {
  const terminalStore = useTerminalStore();
  const unlistenFns: Array<() => void> = [];

  onMounted(async () => {
    unlistenFns.push(
      await listen<GitOpOutputEvent>(GIT_OP_EVENTS.OUTPUT, (event) => {
        terminalStore.onGitOpOutput(event.payload);
      }),
      await listen<GitOpStartedEvent>(GIT_OP_EVENTS.STARTED, (event) => {
        terminalStore.onGitOpStarted(event.payload);
      }),
      await listen<GitOpFinishedEvent>(GIT_OP_EVENTS.FINISHED, (event) => {
        terminalStore.onGitOpFinished(event.payload);
      }),
    );
  });

  onUnmounted(() => {
    for (const un of unlistenFns) un();
    unlistenFns.length = 0;
  });
}
