# PAF-17 GitGraph 冲突横幅红底红字不可见

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-21，核查智能体验证 |
| 关联任务 | D-07（themeOverrides）、F-37（同类对比度问题） |

## 问题描述

`src/views/GitGraph.vue:636-652`：`.conflict-bar` 的
`background: var(--gw-danger)`（:641）与 `.conflict-text` 的
`color: var(--gw-danger)`（:648）同色——`--gw-danger` 为不透明色
（tokens.scss:18 `#ff3b30` / 暗色 :80 `#f87171`），冲突提示文字完全不可见。
模板 :30-33 中 `.conflict-text` 是 `.conflict-bar` 直接子元素。

## 定位与修复建议

- 文字改用 `--gw-danger` 前景对比色（如白色）或背景改 danger-soft token；
- 亮/暗主题下都验证对比度（参照 F-37 的处理方式）。

## 验收标准

- [x] 存在冲突时 GitGraph 顶部横幅文字在亮/暗主题下均可读（danger 文字 + 12% 透明度危险底，同 RuntimeDashboard 既有 soft 惯用法）
- [x] `pnpm build` 通过（vue-tsc --noEmit + vite build）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立（tokens 无 danger-soft/on-danger 前景 token，故未走新增 token 路线）。`.conflict-bar` 背景改 `color-mix(in srgb, var(--gw-danger) 12%, transparent)`、下边框 35%（复用 `RuntimeDashboard.vue:1012` 项目既有 soft-danger 惯用法，不硬编码色值），`.conflict-text` 保持 `--gw-danger` 并加 `font-weight: 500`——亮色 `#ff3b30`、暗色 `#f87171` 文字落在近透明底上均满足对比度。`.conflict-hint` 的 `--gw-text-dim` 在 soft 底下恢复可读，无需改动。验证：`pnpm build` 通过。 |
