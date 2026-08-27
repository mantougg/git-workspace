# D-03 骨架组件（AppShell / SideNav / StatusBar）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §4 全章 + §5.1 / §5.2 / §5.3（布局基准，严格对照）；直接依赖：[D-01](./D-01-design-tokens.md)。版本槽位数据源见根 `AGENTS.md` F-07 条目。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · Desktop Shell |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | D-01 |
| 对应方案 | §4 / §5.1 / §5.2 / §5.3 |

## 目标

实现桌面壳层三件套 `src/components/shell/`：`AppShell`（组合件）、`SideNav`（左侧导航）、`StatusBar`（底部状态栏）。样式全部使用 D-01 tokens，禁止硬编码色值/像素。

## 需求范围

### AppShell

- [ ] 组合 SideNav + 内容区（`<router-view>` 插槽）+ StatusBar，结构尺寸按 §5.1
- [ ] SideNav 与内容区之间 1px `--gw-border` 边框，无空隙无阴影；内容区背景 `--gw-bg-app`
- [ ] AppShell 自身不滚动，滚动归视图内容区

### SideNav（§5.2）

- [ ] 产品名区（13px 半粗、高 40px、下边框 1px）
- [ ] 四个分组（工作区 / Git / Runtime / 设置），全量平铺、图标 16px + 文字 13px、行高 32px，条目来自 router meta（D-04 提供，本任务可先内置等价的静态配置，D-04 时切换为 meta 驱动）
- [ ] 选中态：左侧 2px `--gw-accent` 指示条 + `--gw-bg-hover` 背景；当前路由自动高亮
- [ ] 折叠 188px ⇄ 48px（底部折叠按钮），折叠态仅图标 + tooltip，折叠状态写 localStorage
- [ ] 任务型页面（diff-viewer / conflict-resolver / runtime-app-wizard）不高亮任何条目

### StatusBar（§5.3）

- [ ] 高 24px、字号 11px、上边框 1px，槽位左→右：工作区｜分支｜watcher｜任务数｜（弹性占位）｜版本
- [ ] 工作区槽位：点击弹出切换器（全部工作区 + 「管理工作区…」入口跳转 `workspaces`）
- [ ] 分支槽位：仅 Git 类视图显示，无上下文隐藏
- [ ] watcher 槽位：绿点/灰点 + tooltip 监听仓库数
- [ ] 任务数槽位：点击 `taskStore.togglePanel()`；无任务时显示「无任务」不可点击
- [ ] 版本槽位：`v<__APP_VERSION__> by <__APP_AUTHOR__>`，永远最右
- [ ] 主题切换**不做进状态栏**（D-02 放设置区）

## 验收标准

- [ ] 三个组件按 §5.1/§5.2/§5.3 布局图渲染，尺寸与 tokens 一致
- [ ] SideNav 折叠/展开正常且状态持久化；路由切换高亮正确
- [ ] StatusBar 五个槽位语义完整（工作区切换器可用、任务槽位可唤起 TaskPanel）
- [ ] 组件内无硬编码色值/像素间距（全部 tokens）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 一期-3） |
| 2026-08-27 | ✅ | 完成：AppShell + SideNav + StatusBar 三组件，pnpm build 通过 |
