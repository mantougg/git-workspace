# D-15 变更视图三栏联动（仓库树 + 提交图 + diff）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §5.5 `[三期]` 项；直接依赖：[D-10](./D-10-panel-toolbar.md)。**三期为可选阶段，启动本任务前先与用户确认范围并细化验收标准。**

| 项 | 值 |
|---|---|
| 阶段 | 三期 · Git Client Experience（可选） |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | D-10 |
| 对应方案 | §5.5 / §6 三期 |

## 目标

把「变更与批量操作」视图从「树 + 内联 diff」升级为 Git 客户端式三栏联动：仓库树 / 提交图 / diff 三块联动（选中仓库 → 提交图过滤；选中提交 → diff 显示该提交），参照 Fork / IDEA Git 工具窗口。

## 需求范围（启动时细化）

- [ ] 三栏布局：左仓库树（复用现有变更树）、中提交图（复用 CommitGraph）、右 diff（复用 diff-pane）
- [ ] 联动：选中仓库过滤提交图；选中提交/文件切换 diff 内容
- [ ] 保留现有批量操作能力（复选树 + commit-panel），联动不破坏批量流
- [ ] 列宽可拖拽（splitter），位置记忆由 D-16 提供

## 验收标准（启动时细化）

- [ ] 三栏联动行为正确，批量操作回归通过
- [ ] 大数据仓库下性能不退化（沿用现有虚拟滚动）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-12 交互修正（提交图改为显式唤起，见时间线）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 三期-1） |
| 2026-08-27 | ✅ | 完成：三栏联动布局（树 + 提交图 + diff），单仓库选中自动加载提交图 |
| 2026-08-28 | ✅ 补齐 | 核查发现 onCommitSelect 为 console.log 占位、三栏未真正联动。补齐：选中提交 → DiffViewer 显示该提交变更（repo+commit query）；graph-pane 提交节点右键（Copy hash/查看 Diff）。pnpm build 通过 |
| 2026-09-12 | ✅ 交互修正 | 移除「勾选节点自动弹出提交图」（勾选=stage/commit 操作意图，不应触发浏览）。改为显式唤起：graphRepoPath 与 treeSelection 解耦；repo 行尾 hover 图标 + 右键菜单「在侧栏预览提交图」toggle 开关；pane header 加「完整页面 / 关闭」按钮；工作区切换或仓库移出时静默关闭；提交选中/右键 Diff 改用 pane 钉住的 graphRepoPath。pnpm build（vue-tsc）通过 |
| 2026-09-13 | ✅ 规范 | AGENTS.md「组件规范」新增：交互控件一律 naive-ui、禁止原生 button/input 自绘（含 h() 内），存量视为技术债顺手替换。ChangeTree 行尾预览入口按规范从原生 button 改为 NButton（text/tiny），颜色密度交 themeOverrides。pnpm build 通过 |
