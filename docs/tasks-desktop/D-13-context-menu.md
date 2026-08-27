# D-13 ContextMenu（变更树 / 提交图右键菜单）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §5.5 / §5.6 的 `[2.5期]` 项；直接依赖：[D-12](./D-12-command-palette.md)（菜单项复用命令注册表）。

| 项 | 值 |
|---|---|
| 阶段 | 2.5 期 · Desktop Interaction |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | D-12 |
| 对应方案 | §5.5 / §5.6 |

## 目标

为变更树与提交图提供右键上下文菜单——单这一项即显著增强「桌面客户端」观感。

## 需求范围

- [ ] `src/components/shell/ContextMenu.vue`：基于 `n-dropdown` 的薄封装（触发位置、菜单项、禁用态）
- [ ] 变更树仓库节点右键：Fetch / Pull / Push / 提交 / 健康检查（单仓库）/ 在文件管理器显示等（只编排已有能力）
- [ ] 变更树文件节点右键：Stage / Unstage / Discard（危险确认）/ 查看 Diff
- [ ] 提交图提交节点右键：Checkout / Reset / Cherry-pick / Copy hash 等，复用现有 `@action` 分发逻辑
- [ ] 菜单项接入命令注册表（有对应命令的复用，无的注册为新命令）

## 验收标准

- [ ] 变更树仓库/文件节点、提交图提交节点右键菜单可用，命令行为与按钮操作一致
- [ ] 危险操作（Discard / Reset hard 等）保留原有确认流
- [ ] 亮/暗下菜单样式正确
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 2.5期-2） |
| 2026-08-27 | ✅ | 完成：ContextMenu 组件 + 变更树右键菜单（Fetch/Pull/Push/Stage/Discard 等），pnpm build 通过 |
| 2026-08-28 | ✅ 补齐 | 核查发现菜单选中为 console.log 空壳、提交图未接入。补齐：变更树 repo 节点 Fetch/Pull/Push/预选提交/健康检查(单仓库)/文件管理器/提交图跳转，file 节点 Stage/Discard(危险确认)/查看 Diff（Unstage 无文件级后端能力，不提供假入口）；CommitGraph 新增 contextmenu 事件，GitGraph 右键复用 T-13 确认流 + Copy hash/查看 Diff，RepositoryList graph-pane 轻量右键。pnpm build 通过 |
