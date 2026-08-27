# D-14 键盘快捷键体系

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §6 2.5期-3 / §7（命令与快捷键统一走命令注册表）；直接依赖：[D-12](./D-12-command-palette.md)。

| 项 | 值 |
|---|---|
| 阶段 | 2.5 期 · Desktop Interaction |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | D-12 |
| 对应方案 | §6 2.5期 / §7 |

## 目标

为导航与高频操作提供键盘快捷键，全部走命令注册表的「命令 id → 按键」映射，视图内不各自绑定 keydown。

## 需求范围

- [ ] 按键映射表（集中一处定义）：导航 `Ctrl/Cmd+1..9`（按 SideNav 顺序）、刷新当前视图、Command Palette（D-12 已有）等
- [ ] 全局 keydown 监听单点实现，按映射表分发到命令注册表
- [ ] 输入框聚焦时不触发单键快捷键（`Ctrl/Cmd` 组合键除外）
- [ ] Command Palette 结果列表中展示命令对应快捷键提示
- [ ] 占用浏览器保留键（如 `Cmd+R`）需逐案评估，不盲目 preventDefault

## 验收标准

- [ ] 映射表内全部快捷键可用，且行为与命令面板执行一致
- [ ] 视图内无独立 keydown 快捷键绑定（grep 验证）
- [ ] 输入中文/表单场景不误触发
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 2.5期-3） |
| 2026-08-27 | ✅ | 完成：快捷键映射表 + 全局监听 + CommandPalette 展示快捷键，pnpm build 通过 |
| 2026-08-28 | ✅ 补齐 | 核查发现缺输入框聚焦守卫。shortcuts.ts 增加 isEditableTarget 守卫（input/textarea/select/contenteditable），pnpm build 通过（commit 5bf64c6） |
