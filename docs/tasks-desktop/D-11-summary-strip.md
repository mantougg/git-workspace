# D-11 Dashboard / Runtime 摘要行收敛 + 自定义视觉件

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §5.4 / §5.7（含评审修订：去卡片墙）；直接依赖：[D-10](./D-10-panel-toolbar.md)。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Desktop Visual System |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-10 |
| 对应方案 | §5.4 / §5.7 |

## 目标

消除「Web Dashboard」观感：Dashboard 8 张 stat-card 与 Runtime stat-card 收敛为高密度摘要行；健康分、热力图等自定义视觉件按 tokens 收敛。

## 需求范围

- [ ] Dashboard 卡片墙（`DashboardView.vue` `.cards` 8 张 stat-card）改为一条摘要行：数字 + 标签平铺、无卡片边框、无大圆角、无阴影
- [ ] **保留点击跳转语义**：每个指标点击跳转变更视图并预填 `@status:xxx` 选择器（现有 `openCard` 语义不变；「未跟踪」原不可跳转则保持不可点击）
- [ ] Runtime 总览 stat-card 同步收敛为摘要行
- [ ] 健康分大字、状态分布条、热力图、我的应用卡片按 tokens 收敛（圆角 `--gw-radius-md`、1px 边框、`--gw-*` 色值）
- [ ] 不做信息流式 Overview / Recent Activity（无后端数据源，评审结论）

## 验收标准

- [ ] Dashboard / Runtime 无卡片墙，摘要行密度符合 §5.4 布局图
- [ ] 摘要行点击跳转与改造前行为一致（`@status:clean` 等预填正确）
- [ ] 亮/暗下各自定义视觉件无破版
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 二期-5） |
