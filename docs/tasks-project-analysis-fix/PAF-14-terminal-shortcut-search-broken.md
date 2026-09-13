# PAF-14 终端快捷键与命令失效三连（Ctrl+` 死绑定 / 搜索命令无人监听 / 搜索聚焦选择器失效）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] Ctrl+\` 可开关终端面板；Ctrl+Shift+\` 新建 shell
- [ ] 命令面板「终端内搜索」打开搜索条且输入框聚焦
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
