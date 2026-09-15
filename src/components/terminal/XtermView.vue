<!--
  XtermView — xterm.js 封装组件（TM-02，terminal-feature-plan §4.3）。
  挂载/卸载、write(Uint8Array) 暴露、fit 自适应、resize 观察器 → 回调 cols/rows。
  组件懒加载（defineAsyncComponent），不拖慢首屏。
-->

<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, nextTick } from "vue";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { open as tauriOpen } from "@tauri-apps/plugin-shell";
import { useTerminalStore } from "@/stores/terminal";
import { encodeUtf8Base64 } from "@/utils/base64";
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
let searchAddon: SearchAddon | null = null;
let resizeObserver: ResizeObserver | null = null;

// ---------------------------------------------------------------------------
// Theme from --gw-* tokens
// ---------------------------------------------------------------------------

function getXtermTheme(): Record<string, string> {
  const style = getComputedStyle(document.documentElement);
  const bg = style.getPropertyValue("--gw-bg-app").trim() || "#1e1e1e";
  const fg = style.getPropertyValue("--gw-text").trim() || "#cccccc";
  const accent = style.getPropertyValue("--gw-accent").trim() || "#4d8bf5";
  const success = style.getPropertyValue("--gw-success").trim() || "#4ade80";
  const warning = style.getPropertyValue("--gw-warning").trim() || "#f59e0b";
  const danger = style.getPropertyValue("--gw-danger").trim() || "#f87171";
  const info = style.getPropertyValue("--gw-info").trim() || "#38bdf8";

  return {
    background: bg,
    foreground: fg,
    cursor: accent,
    cursorAccent: bg,
    selectionBackground: accent + "33",
    // ANSI 颜色（16色）
    black: "#000000",
    red: danger,
    green: success,
    yellow: warning,
    blue: accent,
    magenta: "#c678dd",
    cyan: info,
    white: fg,
    brightBlack: "#5c6370",
    brightRed: "#e06c75",
    brightGreen: "#98c379",
    brightYellow: "#e5c07b",
    brightBlue: "#61afef",
    brightMagenta: "#c678dd",
    brightCyan: "#56b6c2",
    brightWhite: "#ffffff",
  };
}

function getXtermFontFamily(): string {
  const resolved = getComputedStyle(document.documentElement)
    .getPropertyValue("--gw-font-mono")
    .trim();
  return resolved || "Consolas, monospace";
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
    fontFamily: getXtermFontFamily(),
    fontSize: 13,
    theme: getXtermTheme(),
    cursorBlink: true,
    convertEol: true,
    scrollback: 5000,
  });

  fitAddon = new FitAddon();
  terminal.loadAddon(fitAddon);

  // TM-07：搜索 addon
  searchAddon = new SearchAddon();
  terminal.loadAddon(searchAddon);

  // TM-07：链接识别 addon（URL 可点击，系统浏览器打开）
  // 使用自定义 handler 调用 Tauri shell.open 打开系统浏览器
  terminal.loadAddon(new WebLinksAddon((_event, uri) => {
    tauriOpen(uri).catch((e) => console.warn("Failed to open link:", e));
  }));

  terminal.open(containerRef.value);

  // 验证渲染链路：写入测试行
  terminal.writeln("Terminal ready.");

  // 注册写入回调（store 收到 terminal_output 时直接写入此 xterm）
  terminalStore.registerWriteCallback(props.sessionId, (data: Uint8Array) => {
    terminal?.write(data);
  });

  // 用户输入 → emit
  terminal.onData((data: string) => {
    // 将字符串转为 base64（支持多字节；分块编码避免大输入栈溢出）
    emit("input", encodeUtf8Base64(data));
  });

  // Ctrl+C 智能处理：有选中文本时复制，无选中文本时发送中断信号
  // Ctrl+Shift+C：始终复制选中文本
  terminal.attachCustomKeyEventHandler((event: KeyboardEvent) => {
    // Ctrl+Shift+C：始终复制
    if (event.ctrlKey && event.shiftKey && event.key === "C") {
      const selection = terminal?.getSelection();
      if (selection && selection.length > 0) {
        navigator.clipboard.writeText(selection).catch((e) =>
          console.warn("Failed to copy:", e)
        );
      }
      return false; // 阻止默认行为
    }
    // Ctrl+C（无 Shift）：有选中文本时复制，无选中文本时发送中断信号
    if (event.ctrlKey && event.key === "c" && !event.shiftKey) {
      const selection = terminal?.getSelection();
      if (selection && selection.length > 0) {
        // 有选中文本，复制到剪贴板
        navigator.clipboard.writeText(selection).catch((e) =>
          console.warn("Failed to copy:", e)
        );
        return false; // 阻止默认行为（不发送中断信号）
      }
      // 无选中文本，让默认行为发生（发送中断信号）
      return true;
    }
    return true;
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
  searchAddon = null;
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

// TM-07：搜索方法
function findNext(text: string, options?: { caseSensitive?: boolean; wholeWord?: boolean; regex?: boolean }): boolean {
  return searchAddon?.findNext(text, options) ?? false;
}

function findPrevious(text: string, options?: { caseSensitive?: boolean; wholeWord?: boolean; regex?: boolean }): boolean {
  return searchAddon?.findPrevious(text, options) ?? false;
}

defineExpose({ write, writeString, clear, getTerminal, findNext, findPrevious });
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
