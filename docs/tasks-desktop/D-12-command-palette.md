# D-12 命令注册表 + Command Palette（Ctrl/Cmd+K）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §5.11；直接依赖：[D-04](./D-04-app-shell-integration.md)（需要 router meta 作为导航命令来源）。一期完成后即可插入，不必等二期结束。

| 项 | 值 |
|---|---|
| 阶段 | 2.5 期 · Desktop Interaction |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-04 |
| 对应方案 | §5.11 |

## 目标

建立命令注册表（`src/commands/`）作为导航与高频操作的统一编排层，并提供 Ctrl/Cmd+K 唤起的 Command Palette——同时承载后续的快捷键体系（D-14）。

## 需求范围

- [ ] 命令注册表：每条命令 = `id` + `title` + `group` + `run()`（导航 push 或调用现有 store/api 方法）；**只编排已有能力，不新增业务逻辑**
- [ ] 内置命令：全部导航视图跳转（来自 router meta）、切换工作区、扫描仓库、刷新、Fetch/Pull/Push 全部仓库（跳转变更视图预填）、新建 Change Set、运行健康检查等高频操作
- [ ] `CommandPalette` 组件（`n-modal` + 输入框 + 结果列表）：居中顶部浮层、模糊搜索、键盘上下选择、Enter 执行
- [ ] 全局 `Ctrl/Cmd+K` 唤起（WebView 内 keydown，不依赖系统菜单）；Esc 关闭
- [ ] 空结果态与分组标题展示

## 验收标准

- [ ] Ctrl/Cmd+K 唤起面板，可搜索并执行导航类与操作类命令
- [ ] 命令执行效果与手动操作一致（导航、预填跳转等）
- [ ] 亮/暗下面板样式正确（走 tokens）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 2.5期-1，评审新增） |
