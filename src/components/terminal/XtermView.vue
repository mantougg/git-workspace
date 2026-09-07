<!--
  XtermView — xterm.js 封装组件（TM-02，terminal-feature-plan §4.3）。
  挂载/卸载、write(Uint8Array) 暴露、fit 自适应、resize 观察器 → 回调 cols/rows。
  组件懒加载（defineAsyncComponent），不拖慢首屏。
-->

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, nextTick } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { useTerminalStore } from "@/stores/terminal";
import "@xterm/xterm/css/xterm.css";

// ---------------------------------------------------------------------------
// Props & Emits
// ---------------------------------------------------------------------------

const props = defineProps<{
  /** 会话 ID（用于标识哪个 xterm 实例）。 */
  sessionId: string;
  /** 是否激活（tab 可见时为 true）。 */
  active: boolean;
  /** 初始列数。 */
  cols?: number;
  /** 初始行数。 */
  rows?: number;
}>();

const emit = defineEmits<{
  /** 用户输入（base64 编码的字节流）。 */
  (e: "input", dataBase64: string): void;
  /** 终端尺寸变化。 */
  (e: "resize", cols: number, rows: number): void;
}>();

// ---------------------------------------------------------------------------
// Refs
// ---------------------------------------------------------------------------

const containerRef = ref<HTMLDivElement>();
let terminal: Terminal | null = null;
let fitAddon: FitAddon | null = null;
let resizeObserver: ResizeObserver | null = null;

// ---------------------------------------------------------------------------
// Theme from --gw-* tokens
// ---------------------------------------------------------------------------

function getXtermTheme(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  return {
    background: style.getPropertyValue("--gw-bg-app").trim() || "#1e1e1e",
    foreground: style.getPropertyValue("--gw-text").trim() || "#cccccc",
    cursor: style.getPropertyValue("--gw-accent").trim() || "#4d8bf5",
    selectionBackground: style.getPropertyValue("--gw-accent").trim() + "33" || "#4d8bf533",
    // 保留 ANSI 默认色（xterm 内置）
  };
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

onMounted(() => {
  if (!containerRef.value) return;

  const terminalStore = useTerminalStore();

  terminal = new Terminal({
    cols: props.cols ?? 80,
    rows: props.rows ?? 24,
    fontFamily: "var(--gw-font-mono)",
    fontSize: 13,
    theme: getXtermTheme(),
    cursorBlink: true,
    convertEol: true,
    scrollback: 5000,
  });

  fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);

  terminal.open(containerRef.value);

  // 注册写入回调（store 收到 terminal_output 时直接写入此 xterm）
  terminalStore.registerWriteCallback(props.sessionId, (data: Uint8Array) => {
    terminal?.write(data);
  });

  // 用户输入 → emit
  terminal.onData((data: string) => {
    // 将字符串转为 base64（支持多字节）
    const encoder = new TextEncoder();
    const bytes = encoder.encode(data);
    const base64 = btoa(String.fromCharCode(...bytes));
    emit("input", base64);
  });

  // Resize 观察器
  resizeObserver = new ResizeObserver(() => {
    if (fitAddon && terminal && props.active) {
      fitAddon.fit();
      emit("resize", terminal.cols, terminal.rows);
    }
  });
  resizeObserver.observe(containerRef.value);

  // 初始 fit
  nextTick(() => {
    if (fitAddon && terminal) {
      fitAddon.fit();
    }
  });
});

onBeforeUnmount(() => {
  const terminalStore = useTerminalStore();
  terminalStore.unregisterWriteCallback(props.sessionId);
  resizeObserver?.disconnect();
  resizeObserver = null;
  terminal?.dispose();
  terminal = null;
  fitAddon = null;
});

// ---------------------------------------------------------------------------
// Watchers
// ---------------------------------------------------------------------------

/** 激活时 fit（从隐藏恢复）。 */
watch(
  () => props.active,
  (active) => {
    if (active && fitAddon && terminal) {
      nextTick(() => {
        fitAddon!.fit();
      });
    }
  }
);

// ---------------------------------------------------------------------------
// Expose（父组件可调用）
// ---------------------------------------------------------------------------

/** 向 xterm 写入字节流（Uint8Array）。 */
function write(data: Uint8Array) {
  terminal?.write(data);
}

/** 向 xterm 写入字符串。 */
function writeString(data: string) {
  terminal?.write(data);
}

/** 清屏。 */
function clear() {
  terminal?.clear();
}

/** 获取 terminal 实例（供高级用法）。 */
function getTerminal(): Terminal | null {
  return terminal;
}

defineExpose({ write, writeString, clear, getTerminal });
</script>

<template>
  <div ref="containerRef" class="xterm-view" />
</template>

<style scoped>
.xterm-view {
  width: 100%;
  height: 100%;
  background: var(--gw-bg-app);
}

/* xterm 内部样式覆盖：确保使用 design tokens */
.xterm-view :deep(.xterm) {
  padding: var(--gw-space-1);
}
</style>
