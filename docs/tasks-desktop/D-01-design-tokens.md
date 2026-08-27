# D-01 Design Tokens（tokens.scss 亮暗双套）

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §3.1；无依赖，一期第一个任务。

| 项 | 值 |
|---|---|
| 阶段 | 一期 · Desktop Shell |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | — |
| 对应方案 | §3.1 |

## 目标

建立全应用唯一的 Design Tokens 文件 `src/styles/tokens.scss`，**亮/暗双套一次到位**，作为后续所有骨架组件与样式收敛的唯一取值来源。

## 需求范围

- [ ] 新增 `src/styles/tokens.scss`，亮色挂 `:root`，暗色挂 `[data-theme="dark"]`
- [ ] 颜色：`--gw-bg-app` / `--gw-bg-panel` / `--gw-bg-hover` / `--gw-border` / `--gw-text` / `--gw-text-dim` / `--gw-accent` / `--gw-success` / `--gw-warning` / `--gw-danger` / `--gw-info`
- [ ] 间距：`--gw-space-1: 4px` ~ `--gw-space-4: 16px`
- [ ] 字号：`--gw-text-xs: 11px` / `--gw-text-sm: 12px` / `--gw-text-md: 13px` / `--gw-text-lg: 14px`
- [ ] 圆角：`--gw-radius-sm: 2px` / `--gw-radius-md: 4px`
- [ ] 字体栈：等宽栈 `--gw-font-mono: ui-monospace, "Cascadia Mono", "JetBrains Mono", Consolas, monospace`
- [ ] 骨架尺寸：`--gw-statusbar-h: 24px`、`--gw-sidenav-w: 188px`、`--gw-sidenav-w-collapsed: 48px`
- [ ] 在 `main.ts`（或全局样式入口）引入 tokens.scss
- [ ] 本任务只定义变量；`[data-theme="dark"]` 的切换机制由 D-02 实现

## 验收标准

- [ ] tokens.scss 存在且亮/暗双套完整，方案 §5 中引用的所有 token 名称均有定义
- [ ] 暗色套取值与 Naive UI darkTheme 的底色协调（挂 `[data-theme="dark"]` 手工切换目检）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 一期-1） |
