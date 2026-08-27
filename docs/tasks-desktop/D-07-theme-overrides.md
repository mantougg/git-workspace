# D-07 Naive UI 组件级 themeOverrides

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §3.2；直接依赖：[D-01](./D-01-design-tokens.md)。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Desktop Visual System |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-01 |
| 对应方案 | §3.2 |

## 目标

通过 `n-config-provider` 的 `:theme-overrides` 全局收敛 Naive UI 组件的密度与圆角，22 个视图零改动获得「客户端级密度」。

## 需求范围

- [ ] `common`：`borderRadius: 4px`、`borderRadiusSmall: 2px`、`fontSize: 13px`，主色/边框色与 tokens 同值
- [ ] `Button` / `Input` / `Select`：高度收敛到 small 档（约 28px）
- [ ] `DataTable`：行高压到 32px
- [ ] `Card` / `Dialog`：圆角 4px、内边距收敛
- [ ] themeOverrides 集中定义在一处（如 `src/styles/naive-overrides.ts`），亮/暗共用；色值引用 tokens 同值常量，不复制粘贴 hex

## 验收标准

- [ ] 全局组件密度/圆角按 §3.2 生效，逐视图目检无破版
- [ ] 亮/暗两套主题下覆盖均正确
- [ ] 默认密度下 1280×800 首屏信息密度明显提升（对照改造前截图）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 二期-1） |
