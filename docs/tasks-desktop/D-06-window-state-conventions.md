# D-06 窗口状态记忆 + AGENTS.md 约定落地

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §4.3 / §7；根 `AGENTS.md`（平台规范 + F-07 条目）。直接依赖：[D-04](./D-04-app-shell-integration.md)。可与 D-05 并行。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · Desktop Shell |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-04 |
| 对应方案 | §4.3 / §7 |

## 目标

接入窗口尺寸/位置记忆；把 Desktop Skin 的使用约定写进 AGENTS.md，防止后续开发绕过 tokens 与骨架组件。

## 需求范围

- [ ] 接入官方插件 `tauri-plugin-window-state`（Rust 侧 + 前端 capabilities 配置），窗口尺寸/位置重启后恢复
- [ ] 根 `AGENTS.md` 新增「Desktop Skin 约定」小节：新 UI 一律使用 tokens 变量，禁止硬编码色值/像素间距；新面板一律使用 `Panel`/`Toolbar`；密度以 themeOverrides 为准；新页面外壳遵循 plan §5.9 统一模式；工作区切换唯一入口为 StatusBar
- [ ] 更新 AGENTS.md 的 F-07 条目：版本栏展示位置从 `App.vue` `.app-footer` 改为 StatusBar 最右槽位（数据源不变）

## 验收标准

- [ ] 调整窗口尺寸/位置后重启应用，窗口恢复
- [ ] AGENTS.md 新增约定小节且 F-07 条目已更新
- [ ] `pnpm build` 与 `cargo check` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 一期-7/8） |
