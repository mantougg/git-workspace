/**
 * 键盘快捷键体系（T-31）
 * 全部走命令注册表的「命令 id → 按键」映射，视图内不各自绑定 keydown。
 *
 * 命令列表由调用方（App.vue / CommandPalette，均在 setup 期）构建后注入：
 * keydown 事件上下文没有组件实例，useRouter() / useXxxStore() 拿不到值
 * （D-14 隐患修复）。
 */

import type { Command, CommandContext } from "./registry";
import { getAllCommands } from "./registry";

/** 快捷键映射表：命令 id → 按键（可绑定多个） */
const SHORTCUT_MAP: Record<string, string[]> = {
  "nav:dashboard": ["Ctrl+1"],
  "nav:changes": ["Ctrl+2"],
  "nav:health": ["Ctrl+3"],
  "nav:git-graph": ["Ctrl+4", "Ctrl+Shift+G"],
  "nav:branch-manager": ["Ctrl+5"],
  "nav:change-sets": ["Ctrl+6"],
  "nav:pipeline": ["Ctrl+7"],
  "nav:runtime-dashboard": ["Ctrl+8"],
  "nav:workspaces": ["Ctrl+9"],
  "nav:diff-viewer": ["Ctrl+Shift+D"],
  "action:toggle-assistant": ["Ctrl+I"],
  "action:repo-search": ["Ctrl+P", "Ctrl+Shift+F"],
  "action:refresh": ["F5"],
  "action:commit": ["Ctrl+Enter"],
  "action:commit-push": ["Ctrl+Shift+Enter"],
};

/** 输入框聚焦时仍允许触发的组合键（提交 / 刷新语义不与文本输入冲突） */
const EDITABLE_ALLOWED = new Set(["Ctrl+Enter", "Ctrl+Shift+Enter", "F5"]);

/** 展示用快捷键描述（多绑定用 " / " 连接） */
export function getShortcutForCommand(commandId: string): string | undefined {
  const keys = SHORTCUT_MAP[commandId];
  return keys?.join(" / ");
}

/** 为所有命令附加快捷键信息 */
export function getCommandsWithShortcuts(ctx: CommandContext): Command[] {
  return getAllCommands(ctx).map((cmd) => ({
    ...cmd,
    shortcut: getShortcutForCommand(cmd.id),
  }));
}

/** 解析按键事件为字符串 */
function parseKeyEvent(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("Ctrl");
  if (e.shiftKey) parts.push("Shift");
  if (e.altKey) parts.push("Alt");

  // 忽略修饰键本身
  if (["Control", "Meta", "Shift", "Alt"].includes(e.key)) return "";

  // 数字键
  if (/^[1-9]$/.test(e.key)) {
    parts.push(e.key);
    return parts.join("+");
  }

  // 字母键
  if (/^[a-zA-Z]$/.test(e.key)) {
    parts.push(e.key.toUpperCase());
    return parts.join("+");
  }

  // 命名键（Enter / F 系列）
  if (e.key === "Enter" || /^F\d{1,2}$/.test(e.key)) {
    parts.push(e.key);
    return parts.join("+");
  }

  return "";
}

/** 事件目标是否为可编辑区域（输入框聚焦时不触发非组合键快捷键） */
function isEditableTarget(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement | null;
  if (!target) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable
  );
}

/**
 * 全局快捷键监听器。
 * `getCommands` 在每次按键时求值（命令含导航/分组等运行时状态，不宜缓存）。
 */
export function createShortcutListener(
  getCommands: () => Command[]
): (e: KeyboardEvent) => void {
  return function onKeydown(e: KeyboardEvent) {
    // 解析按键
    const keyStr = parseKeyEvent(e);
    if (!keyStr) return;

    // 输入框聚焦时跳过快捷键分发（Ctrl+Enter 提交、F5 刷新除外）
    if (isEditableTarget(e) && !EDITABLE_ALLOWED.has(keyStr)) return;

    // 查找匹配的命令
    const commands = getCommands();
    for (const cmd of commands) {
      const keys = SHORTCUT_MAP[cmd.id];
      if (keys && keys.includes(keyStr)) {
        e.preventDefault();
        void Promise.resolve(cmd.run()).catch(() => {
          // 命令执行失败由调用方（如 Palette）提示；这里兜底吞掉
          // 避免 unhandled rejection。
        });
        return;
      }
    }
  };
}
