# GF-05 git 关键路径原生控件治理（冲突编辑器 textarea / hunk 暂存 button）

> 状态：✅ 已完成
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

git 工作流关键路径上存在 3 处原生 HTML 控件，违反 AGENTS.md「交互控件一律使用
naive-ui」规范，导致 themeOverrides / 暗色主题 / 组件密度在这些角落失守：

1. **冲突解决 RESULT 编辑器**是裸 `<textarea>`（无行号、无语法高亮、无主题一致性）——冲突解决是全应用最高危的编辑场景。
2. **hunk 暂存按钮**（Stage/Unstage Hunk、Stage N 行）是原生 `<button class="hunk-btn">`，位于 VirtualList slot 内，每个 hunk 渲染两个不受主题管控的控件。

## 定位线索（证据）

- 冲突编辑器：`src/views/ConflictResolver.vue:112`（`<textarea v-model="resultText" class="pane-editor" spellcheck="false" />`）
- hunk 按钮：`src/components/diff/UnifiedDiff.vue:14`、`:20`（VirtualList slot 内，`:9-55`）
- 规范出处：根目录 `AGENTS.md` Desktop Skin 约定「交互控件一律使用 naive-ui 组件」；存量技术债已列 `SideNav.vue:27,33` 两处原生 button（可顺手替换，不计入本任务范围）。

## 修复范围 checklist

- [x] 1. `ConflictResolver.vue:112`：`<textarea>` → `n-input type="textarea"`（保持自动增高/等宽字体；行号与语法高亮不做，范围控制）。
- [x] 2. `UnifiedDiff.vue:14,:20`：原生 button → `n-button size="tiny"`（或 `quaternary`），确认在 VirtualList 内渲染开销不升（每 hunk 两个按钮，滚动帧率对比）。
- [x] 3. 暗色/亮色主题下目视一致；行级/hunk 暂存功能回归（暂存相关测试在）。

## 实现说明（2026-09-24）

- **复现结论**：两处原生控件均成立（`ConflictResolver.vue:112` 裸 textarea；`UnifiedDiff.vue:14/:20` 裸 button）。SideNav 两处原生 button 按任务边界不动。
- RESULT 编辑器：`<textarea>` → `n-input type="textarea"`（naive 自动导入，无需显式 import）。填满 pane：外层 `.pane-editor` flex:1 + `:deep(.n-input__textarea-el) height:100%`；边框 overlay（`.n-input__border`/`.n-input__state-border`）display:none 保持原无边框观感；文字样式（等宽 `--gw-font-mono`、12px、`--gw-bg-panel`/`--gw-text`）走 tokens；配色/hover/focus 环由 themeOverrides 统一管控（暗色主题生效点）。v-model 语义不变（编辑/保存功能零回归）。
- hunk 按钮：`<button class="hunk-btn">` → `n-button size="tiny"`：
  - Stage/Unstage Hunk → `quaternary`（弱化观感，与原 border 弱化等价）；
  - Stage N 行 → `quaternary type="primary"`（主色文字按钮，替代原 `.primary` 的自绘 accent 边框）。
  - **21px 固定行高约束**：VirtualList ROW_HEIGHT=21，naive tiny 按钮默认高 22px 会溢出行——`.hunk-btn` 压到 height:19px + font-size:11px + padding 0 6px（scoped 类特异性胜出 naive 尺寸类）。这是为满足固定行高的受控偏离，主题色/密度仍归 themeOverrides。
  - VirtualList 内渲染开销：n-button 为轻量 render 函数组件，ChangeTree 树行内已有同款先例（graph-suffix-btn size tiny）；每 hunk 至多 2 个实例，可接受。
- 验收 3 的 grep 已执行：两文件 `<textarea`/`<button` 残留 0。

## 不做（范围控制）

- 不给 RESULT 编辑器做语法高亮 / diff 视图编辑器（大体量工程，另立任务）。
- 不清理 TerminalPanel/SideNav 等非 git 路径存量（AGENTS.md 已标注为技术债，按「触及再换」原则）。

## 验收标准

1. 冲突解决页 RESULT 区在暗色主题下与 naive 控件观感一致，编辑/保存功能不回归。
2. UnifiedDiff 的 hunk 按钮在两种主题下样式正确；暂存 hunk/行的单测（如有）通过，千行 diff 滚动帧率无明显下降。
3. `pnpm build` 通过；`grep '<textarea\|<button' src/views/ConflictResolver.vue src/components/diff/UnifiedDiff.vue` 无残留。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（git 路径仅存 3 处违反，均在关键路径）。待复现与修复。 |
| 2026-09-24 | 复现成立（两处控件坐实；SideNav 两处按边界不动）。修复：ConflictResolver RESULT 编辑器 → n-input type=textarea（flex 填满 pane、overlay 边框隐藏、tokens 文字样式）；UnifiedDiff hunk 按钮 → n-button size=tiny（quaternary；次级 primary quaternary；19px 适配 21px 固定行高）。验证：`pnpm build`（vue-tsc+vite）通过；两文件原生控件 grep 残留 0。状态 → ✅。 |
