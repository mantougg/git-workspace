import { ref, watch, onMounted, onUnmounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";

export type ThemeMode = "system" | "light" | "dark";

const STORAGE_KEY = "gw-theme-mode";

/** 当前主题模式（用户选择） */
const mode = ref<ThemeMode>(
  (localStorage.getItem(STORAGE_KEY) as ThemeMode) || "system"
);

/** 实际生效的亮/暗（resolved） */
const resolved = ref<"light" | "dark">("light");

let unlistenTheme: UnlistenFn | null = null;

/** 根据 mode 与系统主题计算 resolved，并同步到 DOM + Naive UI */
async function applyTheme(systemTheme?: "light" | "dark" | null) {
  const appWindow = getCurrentWindow();

  if (mode.value === "system") {
    const sys = systemTheme ?? (await appWindow.theme()) ?? "light";
    resolved.value = sys;
    await appWindow.setTheme(null); // 跟随系统
  } else {
    resolved.value = mode.value;
    await appWindow.setTheme(mode.value);
  }

  // 驱动 tokens 暗色套
  const root = document.documentElement;
  if (resolved.value === "dark") {
    root.setAttribute("data-theme", "dark");
  } else {
    root.removeAttribute("data-theme");
  }
}

export function useTheme() {
  /** 设置主题模式（三档） */
  async function setMode(m: ThemeMode) {
    mode.value = m;
    localStorage.setItem(STORAGE_KEY, m);
    await applyTheme();
  }

  onMounted(async () => {
    // 初始化
    await applyTheme();

    // 监听系统主题变化（仅 system 档生效）
    try {
      unlistenTheme = await getCurrentWindow().onThemeChanged(
        ({ payload }) => {
          if (mode.value === "system") {
            applyTheme(payload);
          }
        }
      );
    } catch {
      // 非 Tauri 环境（dev browser）静默忽略
    }
  });

  onUnmounted(() => {
    unlistenTheme?.();
    unlistenTheme = null;
  });

  // mode 变化时重新应用（防御性，setMode 已处理）
  watch(mode, () => applyTheme());

  return { mode, resolved, setMode };
}
