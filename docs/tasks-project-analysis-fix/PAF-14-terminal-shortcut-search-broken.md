# PAF-14 终端快捷键与命令失效三连（Ctrl+` 死绑定 / 搜索命令无人监听 / 搜索聚焦选择器失效）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-15/P1-16/P1-17，主控 + 核查智能体双重验证 |
| 关联任务 | D-12（命令面板）、D-14（快捷键）、TM-07（终端打磨） |

## 问题描述

1. **Ctrl+\` 死绑定**：`src/commands/shortcuts.ts:55-83` `parseKeyEvent` 只
   识别数字/字母/Enter/F 键，反引号返回 `""`——`terminal:toggle` 与
   `terminal:new-shell` 快捷键永远无法触发，而 StatusBar tooltip 仍在宣传
   「终端（Ctrl+\`）」。
2. **「终端内搜索」命令失效**：`commands/registry.ts:239` 派发
   `CustomEvent("terminal:toggle-search")`，全工程 grep 无任何
   addEventListener 接收——派发后无人消费。
3. **终端搜索聚焦彻底失效**（核查中新发现，比原判断更严重）：
   `TerminalPanel.vue:60-64` 用
   `document.querySelector(".terminal-search-input input")`，要求 input
   嵌套在同类元素内，而模板 :321 中 class 就在 `<input>` 自身——选择器
   永远匹配不到，`focus()` 是空操作。

## 定位与修复建议

1. `parseKeyEvent` 支持 Backquote（e.key === '`'）；
2. TerminalPanel 监听 `terminal:toggle-search` 打开搜索条；
3. 聚焦改用 Vue ref，删除 querySelector + setTimeout hack。

## 验收标准

- [x] Ctrl+\` 可开关终端面板；Ctrl+Shift+\` 新建 shell
- [x] 命令面板「终端内搜索」打开搜索条且输入框聚焦
- [x] `pnpm build` 通过（vue-tsc --noEmit + vite build）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核三处证据全部成立。修复：① `parseKeyEvent` 增加反引号分支（`` Ctrl+` `` / `Ctrl+Shift+\` 可解析）；② TerminalPanel `onMounted` 监听 `terminal:toggle-search`（registry 派发终于有人消费），`onBeforeUnmount` 移除；③ 搜索聚焦改模板 `ref="searchInputRef"` + `nextTick`，删除 querySelector + setTimeout hack。连带修复（同根因 `isEditableTarget` 拦截）：xterm 的隐藏 textarea 属可编辑目标，终端聚焦时所有终端快捷键全被吞——`Ctrl+\``/`Ctrl+Shift+\`` 加入 `EDITABLE_ALLOWED`（面板开关语义，任意输入区放行），`Ctrl+F`/`Ctrl+Shift+C`/`Ctrl+Shift+V` 加入 `TERMINAL_EDITABLE_ALLOWED`（仅焦点在 `.xterm` 内放行，不劫持普通输入框）。验证：`pnpm build` 通过。 |
