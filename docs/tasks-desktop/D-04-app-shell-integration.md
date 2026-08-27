# D-04 App.vue 壳层改造 + router meta + TaskPanel 收编

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §4.1 / §5.1 / §5.10；直接依赖：[D-03](./D-03-shell-components.md)。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · Desktop Shell |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | D-03 |
| 对应方案 | §4.1 / §5.1 / §5.10 |

## 目标

把应用入口从「router-view + 版本 footer」改为 AppShell 骨架；router 补充分组 meta 驱动 SideNav；TaskPanel 唤起入口收编到 StatusBar。

## 需求范围

- [ ] `App.vue` 改为 `AppShell > SideNav + router-view + StatusBar`；移除原 `.app-footer`（F-07 版本展示移入 StatusBar，数据源 `__APP_VERSION__` / `__APP_AUTHOR__` 不变）
- [ ] `src/router/index.ts` 每条路由补 meta：`group`（工作区/Git/Runtime/设置/无）、`title`、`icon`、`nav`（是否进 SideNav）
- [ ] SideNav 改为 router meta 驱动（替换 D-03 的静态配置）；任务型路由（`diff-viewer` / `conflict-resolver` / `runtime-app-wizard`）`nav: false`
- [ ] 默认落地视图保持 `dashboard`
- [ ] TaskPanel 业务逻辑与悬浮形态不变，仅唤起入口统一为 StatusBar 任务槽位；各视图内「任务 (n)」按钮的移除在 D-05 统一执行
- [ ] 骨架提供统一「返回」能力（面包屑或标题栏返回），供任务型页面使用；具体视图的旧返回按钮移除在 D-05 执行

## 验收标准

- [ ] 所有 `nav: true` 视图经 SideNav 可达，高亮正确；任务型页面不进导航
- [ ] 启动默认落地 Dashboard
- [ ] TaskPanel 可从 StatusBar 唤起/收起，原有功能不受影响
- [ ] 亮/暗主题下骨架无破版
- [ ] `pnpm build`（含 vue-tsc）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 一期-4/5） |
| 2026-08-27 | ✅ | 完成：App.vue 改用 AppShell，router meta 驱动 SideNav，pnpm build 通过 |
