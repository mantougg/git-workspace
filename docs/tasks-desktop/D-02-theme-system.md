# D-02 主题机制（darkTheme 绑定 / 系统跟随 / 三档持久化）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §3.1 / §3.2；直接依赖：[D-01](./D-01-design-tokens.md)。Tauri API 使用遵守根 `AGENTS.md` 平台规范。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · Desktop Shell |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-01 |
| 对应方案 | §3.1 / §3.2 / §4.2 |

## 目标

让应用具备完整的亮/暗主题能力：默认跟随系统，可手动三档覆盖并持久化；tokens 暗色套与 Naive UI darkTheme 同步切换。

## 需求范围

- [ ] `App.vue` 的 `n-config-provider` 接 `:theme`（`null` / `darkTheme`），由响应式的主题状态驱动
- [ ] 主题状态三档：`system`（默认）/ `light` / `dark`，持久化到 localStorage
- [ ] 系统主题获取与监听：Tauri `appWindow.theme()` 初始化 + `onThemeChanged` 监听，`system` 档下实时跟随
- [ ] 切换暗色时同步在根元素设置/移除 `[data-theme="dark"]`（驱动 tokens 暗色套）
- [ ] 三档切换入口放设置区（SideNav 底部「外观」弹层即可，不做独立页面，不进 StatusBar）
- [ ] 抽成 composable（如 `src/composables/useTheme.ts`），供 AppShell 与设置入口共用

## 验收标准

- [ ] 系统亮/暗切换时（`system` 档）应用实时跟随，tokens 暗色套同步生效
- [ ] 手动三档可覆盖，重启后选择保持
- [ ] 状态栏无主题切换常驻槽位（评审结论）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 一期-2） |
