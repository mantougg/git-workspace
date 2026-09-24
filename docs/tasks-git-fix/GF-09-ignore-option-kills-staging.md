# GF-09 Ignore diff 选项静默禁掉行级暂存（已勾选的行选择无声丢失）

> 状态：✅ 已完成
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

- [x] 1. Ignore 开启时，行选择区**明确禁用**（置灰 + tooltip 说明「Ignore 选项下无法行级暂存」），并在切选项时提示「已清除 N 行选择」——二选一策略（保留选择至关闭 ignore 时恢复，或立即清除并提示），实现时定并在进度记录理由。
- [x] 2. 把「小 tag」升级为显眼提示（n-alert 或 banner），与 Ignore 选项控件就近。
- [x] 3. 评估后端 `git apply --ignore-whitespace` 路径可行性：若低成本可支持 ignore 下行级暂存则做；不可行则保持 UX 层处理，并在任务文档记录结论。

## 实现说明（2026-09-24）

- **复现结论**：成立。`interactiveMode` 在任一 ignore 选项开启时返回 null；文件头仅一行不显眼 `n-tag`；`UnifiedDiff.vue` 点行选中 + hunk 头 Stage/Unstage 交互实现在其内部（selection 为组件私有状态）。
- **关键事实核验**：`watch(diffOptions, () => loadDiff(), { deep: true })`（DiffViewer.vue）——切换 ignore 选项即触发 diff 重载，`selectedFile` 换新对象，UnifiedDiff 的 `watch(() => props.file)` **本来就会清空选择**。即「立即清除」是既有事实，「保留至关闭 ignore 时恢复」需要跨重索引缓存选择（ignore 后 hunk/行号已重排，缓存无意义且危险）。**策略选定：立即清除并提示**（与 reload 语义一致，无隐藏状态）。
- 实现：
  1. `UnifiedDiff.vue` 新增 `selection-change` emit（toggleLine / file 切换时派发当前数量；file 切换派发 0）——不改动既有 `op` emit 与暂存语义。
  2. `DiffViewer.vue`：
     - 小 tag → `n-alert`（warning、small、无边框、紧凑嵌入 file-diff-header 行，就近 staging 区；header 文字外包 `n-tooltip` 说明「Ignore 重排 hunk/行号，暂存按默认 diff 索引执行（T-12 契约），关闭后恢复」）。
     - `watch(ignoreOptionsActive)`：false→true 且 `unifiedSelectedCount > 0` 时 `message.info("Ignore 选项已开启：已清除 N 行选择，行级 / hunk 暂存暂不可用")`。watcher 创建序保证 diffOptions watch（先建）已发起 reload，而 UnifiedDiff 尚未重渲染，读数即切换前真实数量。true→false 不提示（alert 消失 + 按钮回归即恢复信号）。
  3. 后端 `git apply --ignore-whitespace` 路径评估结论：**不做**。理由：① stage_lines 是任意行子集暂存，无法表达为 `git apply` 补丁（需逐行构造 context，与后端 `stage_lines` 的 index 区间语义完全不同源）；② ignore 版 diff 与默认 diff 的 hunk 边界不一致，`git apply` 的成功/失败与「按 ignore 视图理解的操作」不可对照，属语义错配而非低成本增强；③ 与 T-12「不改默认语义」的边界冲突。保持 UX 层处理（本任务），彻底支持需新增「按 ignore 索引暂存」的后端命令族（另立任务评估）。
- 不改 stage_hunk/stage_lines 默认语义（非 ignore 路径行为零变化）；diff 计算本身未动。

## 验收标准

1. 开启 Ignore 后：行选择不可用有明确原因提示（n-alert + tooltip）；此前已勾选的行有「已清除」反馈（`message.info` 含 N）。
2. 关闭 Ignore：行级暂存立即恢复，功能与现状一致。
3. `pnpm build` 通过；行级暂存相关单测（如有）无回归（本项目前端无单测框架，以 build 为验证手段；后端未改动）。

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
| 2026-09-24 | 复现成立。修复：策略定「立即清除并提示」（reload 本就清空选择，保留策略需跨重索引缓存、无意义且危险）；UnifiedDiff 增 `selection-change` emit；DiffViewer 小 tag → n-alert + tooltip（T-12 契约说明）、ignore 开启瞬间 `message.info` 报「已清除 N 行选择」；后端 `git apply --ignore-whitespace` 路径评估为不做（stage_lines 任意行子集无法表达为 apply 补丁 + 语义错配，结论入文档）。验证：`pnpm build`（vue-tsc+vite）通过。状态 → ✅。 |
