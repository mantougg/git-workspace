# GF-09 Ignore diff 选项静默禁掉行级暂存（已勾选的行选择无声丢失）

> 状态：⬜ 未开始
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

Diff 视图开启 Ignore Whitespace / EOL / Case 任一选项后，行级/hunk 暂存会**静默
失效**：`interactiveMode` 直接返回 null，用户此前勾选的行选择全部消失，只在文件头
显示一行不显眼的小 tag。用户感知是「我勾选了行，按钮却不出现/勾选被清了」，且不知道为什么。

## 定位线索（证据）

- 失效逻辑：`src/views/DiffViewer.vue:433-438`（`interactiveMode` 返回 null）
- 唯一提示：`src/views/DiffViewer.vue:211-215`（文件头小 tag「Ignore 选项开启时暂存操作不可用」）
- 交互实现在 `src/components/diff/UnifiedDiff.vue`（点行选中 + hunk 头 Stage/Unstage）
- 后端：`src-tauri/src/commands/diff.rs:209-233`（stage_hunk/unstage_hunk/stage_lines 均无 ignore 参数）
- **根因备注（2026-09-24 核验）**：这是 T-12 的**已知契约**而非疏漏——源码注释
  （`DiffViewer.vue:430-432`）明确记载「Ignore options renumber hunks/lines, which would
  break the indices staging operates on」。修复必须尊重该契约（要么提示/恢复选择，
  要么走 `git apply --ignore-whitespace` 类后端路径），不得直接在前端强行放行。

## 修复范围 checklist

- [ ] 1. Ignore 开启时，行选择区**明确禁用**（置灰 + tooltip 说明「Ignore 选项下无法行级暂存」），并在切选项时提示「已清除 N 行选择」——二选一策略（保留选择至关闭 ignore 时恢复，或立即清除并提示），实现时定并在进度记录理由。
- [ ] 2. 把「小 tag」升级为显眼提示（n-alert 或 banner），与 Ignore 选项控件就近。
- [ ] 3. 评估后端 `git apply --ignore-whitespace` 路径可行性：若低成本可支持 ignore 下行级暂存则做；不可行则保持 UX 层处理，并在任务文档记录结论。

## 不做（范围控制）

- 不改 stage_hunk/stage_lines 的默认语义（非 ignore 路径行为零变化）。
- 不动 diff 计算本身（ignore 选项的后端实现已正常）。

## 验收标准

1. 开启 Ignore 后：行选择不可用有明确原因提示；此前已勾选的行有「已清除」反馈（或关闭 ignore 后恢复，按实现策略）。
2. 关闭 Ignore：行级暂存立即恢复，功能与现状一致。
3. `pnpm build` 通过；行级暂存相关单测（如有）无回归。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（`:431-437` 静默 null，`:209-215` 仅小 tag）。待复现与修复。 |
