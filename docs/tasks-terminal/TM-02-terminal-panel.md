# TM-02 终端面板前端（xterm 封装 + drawer + tab）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.2（IPC 契约，对接基准）/ §4.3（面板设计）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-01（契约）。**契约先行：TM-01 未完成时可先用 mock 数据开发渲染层，联调在 TM-03。**

| 项 | 值 |
|---|---|
| 阶段 | 一期 · 终端基础 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | TM-01（契约） |
| 对应方案 | §4.3 前端面板 / §4.2 IPC 契约 |

## 目标

实现终端面板前端：底部 drawer（仿 TaskPanel）+ 多会话 tab + xterm 封装组件 + store + StatusBar 入口 + 命令注册表条目。本任务完成全部 UI 与状态管理，用 mock 输出验证渲染；真实 PTY 联通在 TM-03 验收。

## 需求范围

### 依赖与组件

- [ ] `package.json` 新增 `@xterm/xterm` + `@xterm/addon-fit`（锁进 pnpm-lock）
- [ ] `src/components/terminal/XtermView.vue`：xterm 封装（挂载/卸载、`write(Uint8Array)` 暴露、fit 自适应、resize 观察器 → 回调 cols/rows）
- [ ] `src/components/terminal/TerminalTabs.vue`：tab 条（新建 shell / 关闭 / 切换 / 会话标题与存活态）
- [ ] `src/components/terminal/TerminalPanel.vue`：底部 drawer + tab 容器，可拖高 240px~85vh（仿 `TaskPanel.vue:232-256`），挂载进 `App.vue` 全局 overlay 区
- [ ] xterm 组件懒加载（`defineAsyncComponent`）

### 状态与入口

- [ ] `src/stores/terminal.ts`：会话列表（kind/title/cwd/alive）、activeTab、panelVisible、每会话写缓冲（合并 chunk 一次性 write）
- [ ] `src/api/terminal.ts`：5 个 command 的 invoke 封装 + `terminal_output`/`terminal_exit` 事件常量与 listen 封装（契约见方案 §4.2）
- [ ] StatusBar 新增「终端」槽位（仿任务槽位，点击 `terminalStore.togglePanel()`）
- [ ] 命令注册表新增 `terminal.toggle` / `terminal.newShell`（含快捷键映射，走 `src/commands/` 注册表，禁止视图内私绑）

### 视觉

- [ ] xterm 主题从 `--gw-*` tokens 取色（背景/前景/光标/选区），跟随亮暗主题切换
- [ ] 字体用 `--gw-font-mono`；面板/tabs 全部 tokens，无硬编码色值/像素

### mock 验证（TM-03 前）

- [ ] 内置 mock 会话：周期写入含 ANSI 颜色、`\r` 进度、中文的样例字节流，验证渲染与缓冲上限逻辑
- [ ] tab 隐藏时暂停渲染（保留数据），切回时补写

## 验收标准

- [ ] 面板可从 StatusBar 槽位与命令注册表两种方式开合；多 tab 新建/切换/关闭正常
- [ ] mock 流下 ANSI 颜色、`\r` 进度行、中文渲染正确；tab 切换无内容丢失
- [ ] 拖高、主题切换、等宽字体符合 desktop-skin 约定；无硬编码色值/像素
- [ ] `pnpm build`（含 vue-tsc）通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-08 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 一期-2） |
