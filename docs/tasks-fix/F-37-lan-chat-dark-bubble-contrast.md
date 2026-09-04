# F-37 LAN 加密聊天深色模式对方消息气泡文字看不清

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-04 用户反馈（工具箱 LAN 加密聊天第 2 项） |
| 关联任务 | F-10（Desktop Skin tokens 规范） |

## 问题描述

深色模式下，对方发来的消息气泡中文字看不清：气泡背景色与文字颜色都是深色。

## 定位线索

- `src/views/toolbox/tools/LanChatTool.vue` `.msg-bubble` 只设置了
  `background: var(--gw-bg-panel)`（深色模式为 `#252526`），**没有设置
  `color`**，文字颜色沿继承链落到浏览器默认（近黑）。
- 全局样式（`src/App.vue`）不设置正文颜色；项目惯例是每个组件显式
  `color: var(--gw-text)`（StatusBar / SideNav / PanelHeader 等均如此），
  该气泡漏了。
- 己方气泡 `.msg-row.mine .msg-bubble` 已显式 `color: var(--gw-bg-panel)`，
  不受影响。

## 修复范围

- [x] `.msg-bubble` 增加 `color: var(--gw-text)`（tokens 合规，不硬编码色值）

## 验收标准

- [x] 深色模式下对方消息气泡文字为 `--gw-text`（#cccccc），与背景对比正常
- [x] 亮色模式表现不变；己方气泡样式不变（已有自己的 color 覆盖）
- [x] `pnpm build`（vue-tsc + vite build）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-04 修复完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-04 | ⬜ | 用户反馈录入：深色模式对方消息气泡背景与字色同为深色，文字看不清 |
| 2026-09-04 | 🟦 | 开始修复 |
| 2026-09-04 | ✅ | 根因：`.msg-bubble` 未显式设置 `color`，深色模式继承到浏览器默认近黑色；修复：补 `color: var(--gw-text)`。验证：`pnpm build` 通过 |
