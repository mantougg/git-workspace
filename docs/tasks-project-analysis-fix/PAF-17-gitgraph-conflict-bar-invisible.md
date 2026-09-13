# PAF-17 GitGraph 冲突横幅红底红字不可见

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 存在冲突时 GitGraph 顶部横幅文字在亮/暗主题下均可读
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
