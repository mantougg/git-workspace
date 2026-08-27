# D-08 `--el-*` 残留与硬编码色值全局替换

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §1（现状诊断）/ §3.1；直接依赖：[D-01](./D-01-design-tokens.md)。可与 D-07 并行。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Desktop Visual System |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | D-01 |
| 对应方案 | §3.1 |

## 目标

清除 Element Plus 遗留的 `--el-*` CSS 变量（当前引用的是不存在的变量）与各视图硬编码色值，全部替换为 D-01 tokens——这是暗色主题在存量视图上生效的前提。

## 需求范围

- [ ] 全局 grep `--el-`，逐处替换为语义对应的 `--gw-*`（如 `--el-color-success` → `--gw-success`、`--el-border-color` → `--gw-border`、`--el-text-color-secondary` → `--gw-text-dim`）
- [ ] 硬编码色值（`#ebeef5` / `#909399` / `#18a058` 等）替换为 tokens
- [ ] 硬编码间距/圆角顺手收敛为 `--gw-space-*` / `--gw-radius-*`（仅样式替换，不改结构）
- [ ] 逐视图目检亮/暗两套无破版

## 验收标准

- [ ] `grep -r "\--el-" src/` 无残留
- [ ] 视图样式中无新增硬编码色值；存量主要硬编码已 token 化
- [ ] 亮/暗切换下各视图颜色正确（重点：Dashboard 统计卡片、GitGraph conflict-bar、健康分色）
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 二期-2） |
| 2026-08-27 | ✅ | 完成：全部 --el-* 替换为 --gw-*，硬编码色值 token 化，pnpm build 通过 |
