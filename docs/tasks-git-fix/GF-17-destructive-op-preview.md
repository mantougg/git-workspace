# GF-17 破坏性单仓操作无结构化预演（merge/rebase/reset 仅文案确认）

> 状态：✅ 已完成
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

危险操作目前靠 UI 文案 + 二次确认兜底（reset --hard 有 Dangerous confirm + 恢复提示），
但**没有「操作会发生什么」的结构化预演**——reset --hard 前看不到将丢弃哪些提交和文件，
merge 前看不到将并入哪些提交。而后端已有成熟的 dry-run 先例（`batch_dry_run` 纯本地
预演 + 单测），把这个模式扩展到单仓破坏性操作是低风险高收益的。

## 定位线索（证据）

- 已有预演模式：`src-tauri/src/commands/batch.rs:240-303`（rayon 并行、merge_commits 内存预测冲突、不改仓库状态、含完整单测）
- 现危险确认：`src/views/GitGraph.vue:442-454,:508-522`（reset 三模式 + hard 的 Dangerous confirm）
- Reflog 的 Restore State 同样直接 hard reset：`src/views/Reflog.vue:118,:259-280`
- 规范依据：Roadmap §46（危险操作须明确 Repository/Branch/Files/Potential Data Loss，`docs/GitWorkspace 产品需求与技术架构 Roadmap.md:1518-1566`）

### 核验结论（2026-09-24 复现，行号已漂移、问题均仍成立）

- **成立（行号漂移）**：`batch_dry_run` 预演模式现行址为 `src-tauri/src/commands/batch.rs:385-481`
  （`dry_run_repo` 实现），rayon 并行 + 内存 `merge_commits` 冲突预判 + 不改仓库状态 +
  完整单测（`dry_run_categorizes_pull_and_push` 等 6 个）均在；本次 preview 命令沿用同一模式。
- **成立（行号漂移）**：GitGraph reset 三模式确认框 + hard Dangerous confirm 现行址为
  `src/views/GitGraph.vue:100-118`（reset n-modal）与 `:555-585`（confirmReset hard 二次确认）；
  右键菜单另有 `reset-hard` 直开 hard 模式入口（`:468-475`）。
- **成立（行号漂移）**：Reflog 的 Restore State 直连 `resetTo(..., "hard")` 现行址为
  `src/views/Reflog.vue:146-150`（resetDialog）与 `:310-334`（handleRestore，纯文案 dialog.error）。
- **新发现（不影响本任务范围）**：merge 的唯一 UI 入口在 `src/views/BranchManager.vue:178-195`
  （merge n-modal）+ `:1258-1288`（runMerge Warning 确认），同样只有文案；本次一并接入预演
  （该文件不在禁改清单，属完成验收标准 2 的最小必要接线，改动仅模板 + 状态 + 确认文案）。

## 修复范围 checklist

- [x] 1. 后端 preview 命令（纯本地、只读）：`preview_reset`（将丢弃的提交清单 + 工作区变更清单）、`preview_merge`（将并入的提交 + 受影响文件 + 冲突预判）。
- [x] 2. rebase 预演评估：**本次不做 rebase 预演，只做 reset/merge 两个**，评估过程与理由见下方「rebase 预演结论」。
- [x] 3. 前端确认框改造：从纯文案升级为「将发生什么」结构展示（提交数/文件数/不可恢复项标红），沿用现有 confirm 组件体系（GitGraph/Reflog 的 n-modal + BranchManager merge n-modal）。

## rebase 预演结论（2026-09-24 评估）

- **可行性**：`core/rebase.rs` 的 `list_rebase_commits` 已能零成本列出将变基的 commit 序列
  （revwalk `upstream..HEAD`），interactive rebase 的 RebaseDialog 本来就在展示该序列——
  「序列」信息不缺。缺的只有「每步冲突概率」。
- **成本与保真度**：每步冲突预判可用内存 `merge_trees`（base=父提交树、ours=onto 树、
  theirs=该提交树，cherry-pick 的三方合并语义）实现，且不落盘；但**累计**精确模拟需要把上一步
  的合并结果树写回对象库（违背只读约束），只能按 onto 顶点逐步独立预判，对「同序列多提交改同
  一文件」的场景会高估/低估冲突——作为概率信号可用、作为承诺不够诚实。
- **决策：不做**，理由：① 任务书允许「成本高则先做 reset/merge 两个」；② rebase 的唯一 UI
  （`src/components/branch/RebaseDialog.vue`）在本次文件边界外，做了也没有消费者（死命令）；
  ③ reset/merge 已覆盖 Roadmap §46 Dangerous 清单中的 Reset --hard 与 Warning 级的 Merge。
  建议后续任务：rebase 确认框改造时一并做逐步预判（按 onto 独立评估 + 明示「近似」语义）。

## 不做（范围控制）

- 不做「预演后可编辑策略」（如预演里改 merge flag）。
- 不改 batch_dry_run 本身（GF-15 消费）。
- 不做 rebase 预演命令（结论见上；不引入无消费者的 IPC 命令）。

## 验收标准

1. reset --hard 前：预览列出将被丢弃的提交（oid+message）与文件变更清单。
2. merge 前：预览列出将并入提交数、受影响文件数、是否预判冲突。
3. 预演命令不改仓库任何状态（单测断言 ref 不变）；`cargo test --lib`（GW_TEST_MANIFEST=1）+ `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：dry-run 模式向单仓破坏性操作扩展（静态核验 + 规范依据 Roadmap §46）。待复现与修复。 |
| 2026-09-24 | 修复完成。根因：危险确认只有文案、无「操作会发生什么」的结构化事实（Roadmap §46 要求 Repository/Branch/Files/Potential Data Loss）。修法：后端新增 `core/preview.rs`（纯本地只读：revwalk 列丢弃/并入提交、`diff_tree_to_tree` 列受影响文件、`statuses` 列工作区变更（排除未跟踪文件，与 reset 语义一致）、内存 `merge_commits` 冲突预判（沿用 batch_dry_run 模式）；列表按 50/200 封顶、计数字段为权威值）；`commands/preview.rs` 新增同步命令 `preview_reset` / `preview_merge`（lib.rs 注册，F-43：同步命令只做只读 git 调用，不 spawn tokio）；ipc_golden 注册 4 个新 IPC 类型并重生成快照。前端 `api/preview.ts` + `types/preview.ts`；GitGraph reset 三模式框、Reflog Reset Here / Restore State（Restore State 由纯文案 dialog.error 改走带预演的确认框、锁定 hard）、BranchManager merge 框（完成验收 2 的 UI 接线；该文件不在禁改清单）全部升级为结构化预展示（提交数/文件数/不可恢复项 `--gw-danger` 标红），hard 二次确认正文附带预演事实。验证：`GW_TEST_MANIFEST=1 cargo test --lib` = 1017 passed / 16 failed / 3 ignored，16 个失败全部命中预存在环境失败清单（real_maven×10、real_node_vite×1、pty smoke×1、node workspace×2、pathutil 大小写×1、benchmark smoke×1），无新增失败；新增 3 个 preview 单测全绿（含 ref/状态不变断言）；`pnpm build`（vue-tsc --noEmit + vite build）通过；`GW_UPDATE_GOLDEN=1` 重生成 golden 后 2 个 ipc_golden 测试通过。 |
