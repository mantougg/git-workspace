/**
 * 键盘快捷键体系
 * 全部走命令注册表的「命令 id → 按键」映射，视图内不各自绑定 keydown。
 */

import { getAllCommands, type Command } from "./registry";

/** 快捷键映射表：命令 id → 按键描述 */
const SHORTCUT_MAP: Record<string, string> = {
  "nav:dashboard": "Ctrl+1",
  "nav:changes": "Ctrl+2",
  "nav:health": "Ctrl+3",
  "nav:git-graph": "Ctrl+4",
  "nav:branch-manager": "Ctrl+5",
  "nav:change-sets": "Ctrl+6",
  "nav:pipeline": "Ctrl+7",
  "nav:runtime-dashboard": "Ctrl+8",
  "nav:workspaces": "Ctrl+9",
};

/** 获取命令的快捷键描述 */
export function getShortcutForCommand(commandId: string): string | undefined {
  return SHORTCUT_MAP[commandId];
}

/** 为所有命令附加快捷键信息 */
export function getCommandsWithShortcuts(): Command[] {
  return getAllCommands().map((cmd) => ({
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

  return "";
}

/** 全局快捷键监听器 */
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

export function createShortcutListener() {
  return function onKeydown(e: KeyboardEvent) {
    // 输入框聚焦时跳过快捷键分发（当前映射表全部为 Ctrl 组合键，
    // 浏览器在输入框内对 Ctrl+数字 无输入语义，但守卫是 spec 要求的机制）
    if (isEditableTarget(e)) return;

    // 解析按键
    const keyStr = parseKeyEvent(e);
    if (!keyStr) return;

    // 查找匹配的命令
    const commands = getAllCommands();
    for (const cmd of commands) {
      const shortcut = SHORTCUT_MAP[cmd.id];
      if (shortcut && shortcut === keyStr) {
        e.preventDefault();
        cmd.run();
        return;
      }
    }
  };
}
