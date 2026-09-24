# GF-16 Undo 覆盖面窄 + 冲突解决日志刷屏

> 状态：✅ 已完成
> 优先级：P1（违反 Roadmap §46「可恢复」原则与 00-约束 Safety First，属安全感知缺口）
> 来源：2026-09-24 Git 使用体验全景盘点；需求依据 project-analysis:239 / :194。

## 问题描述

Operation Log / Undo 体系（`preview_undo_operation` / `undo_operation`，基于 ref 快照）
已建成且 OperationLogView 已消费，但覆盖面有明显缺口：

1. **白名单窄**：stash drop/clear、merge abort、cherry-pick、worktree remove、
   batch_restore 均不入日志——这些恰是高风险操作，用户做完发现错了没有后悔药。
2. **冲突解决日志刷屏**：一次 merge/rebase 冲突解决会为**每个文件**生成一条独立操作
   日志，且冲突解决明确不可撤销——Operation Log 被几十条同源记录刷屏，真正想找的
   那次操作反而找不到。
3. 批量 branch op 的 `after_oid` 恒为 None（`src-tauri/src/commands/batch.rs:139-141`，
   代码注释自述原因：任务队列异步执行，提交时 after 态未知），撤销预览只能给 before 态。

**核验备注（2026-09-24）**：三条全部坐实，其中两处是「有注释的已知取舍」而非疏漏——
`batch.rs` 的 after_oid None 有设计注释（异步入队时机问题，可改为任务完成后回填）；
`conflict.rs:57-59` 注释明确「conflict resolution 不可经 T-34 Undo 撤销，恢复途径是
当前操作的 Abort 流程或手动编辑」。本任务的价值正在于把这些「注释里的妥协」变成
可撤销/不刷屏的实现。

## 定位线索（证据）

- 撤销体系：`src-tauri/src/commands/operation_log.rs:70,:81`（preview_undo / undo_operation）
  — **复验成立**（2026-09-24，行号未漂移）。
- 覆盖清单（checkout/delete/reset/rebase/restore/conflict-resolution）：同文件白名单匹配，复现时定位具体枚举
  — **复验成立**：白名单即 `core/operation_log/undo_plan.rs::plan_item` 的 match
  （`OP_CHECKOUT_ALL / OP_DELETE_BRANCH_ALL / OP_RESET / OP_REBASE / OP_AI_COMMIT`），
  其余 op_type 一律落入 `other => Err("操作类型 '…' 不支持撤销")`；`OP_CONFLICT_RESOLUTION`
  与 `OP_RESTORE_FILES` 有日志行但**不可撤销**。
- batch after_oid 恒 None：`src-tauri/src/commands/batch.rs:141`
  — **复验成立（轻微漂移）**：Checkout 臂注释在 `batch.rs:140-141`，`after_oid: None` 在
  `batch.rs:142`；Delete 臂同样恒 None。
- 冲突逐文件日志：`src-tauri/src/commands/conflict.rs:58-81`（每次 resolve 动作落一条）
  — **复验成立**：`record_conflict_resolution` 每次调用一条日志；`conflict.rs:55-57`
  注释明确「T-34 Undo 是 ref 快照模型，冲突解决不可撤销」。另发现同一刷屏路径还存在于
  `task/worker.rs:459-471`（AI 提案的 ConflictApply 任务，同样每文件一条）。
- stash drop/clear（commands/stash.rs）、merge abort（commands/merge_rebase.rs）、
  cherry-pick（commands/history.rs）、worktree remove（commands/worktree.rs）：
  **均无任何 operation_log 埋点**（复验成立，无日志即无撤销入口）。
- batch restore（RestoreFiles）：`task/worker.rs:483-497` 有 T-34 埋点但走
  「HEAD 快照」模型——restore 不动 ref，快照无法回退工作区改动（复验成立）。

## 修复范围 checklist

- [x] 1. 评估并扩展 undo 白名单：stash drop/clear、merge abort、cherry-pick、worktree remove、batch_restore 按「是否有可靠 ref 快照可回退」逐项评估（结论见下方「白名单评估」表；不可靠的明确不做并在文档记录原因）。
- [x] 2. 冲突解决合并日志：一次 merge/rebase 的连续 resolve 合并为**一条**操作日志（含变更文件清单与前后 ref），不再逐文件刷屏。
- [x] 3. batch_branch_op 记录 after_oid（批量 checkout/create/delete 后逐仓回填），撤销预览给完整前后态。
- [x] 4. 撤销不可覆盖的操作（stash clear 若确定不回退、conflict_resolution、restore_files）在日志/预览中明示「不可撤销」，管理用户预期。

## 白名单评估（GF-16 checklist 1）

| 操作 | 评估 | 结论 | 依据 |
|---|---|---|---|
| stash drop | **可靠** | ✅ 已实现 | stash 提交对象在 drop 后仍存活（只删 reflog 项），提交前快照整栈（oid+reflog message）即可在 `refs/stash` 重建（新增 `stash::restore_stash_entries`，reflog 原子重建） |
| stash clear | **可靠** | ✅ 已实现 | 同上；clear 会删掉 `refs/stash` ref，恢复时先重建 ref 再重建 reflog（单测覆盖） |
| merge abort | **可靠** | ✅ 已实现 | 快照 MERGE_HEAD + abort 后 HEAD；同提交对的 merge 是确定性的，undo = `merge::restart_merge_at` 重跑合并恢复冲突态；HEAD 有后续变更则拒绝 |
| cherry-pick | **可靠** | ✅ 已实现 | pick 成功后分支只是前移到新提交，硬回退到 pre-pick oid 即精确还原（工作区要求干净，同 rebase 模型）；进行中的 pick 不记录（abort 流程自有恢复） |
| worktree remove | **可靠** | ✅ 已实现 | 快照 name/path/分支或 detached oid；undo = `worktree::restore_worktree` 重建（detached 经临时引用 + 检出后 detach + 删临时引用实现）；**未提交改动不可恢复**（删除时即丢失），预览文案明示 |
| batch create | **可靠（附带）** | ✅ 已实现 | 任务原文「create 不入日志」的取舍在 GF-16 有反悔药后失效：撤销 = 删除创建的分支（tip 未变时安全），故一并记录 + 回填 |
| batch restore（RestoreFiles） | **不可靠** | ❌ 明确不做 | restore 不动 ref，丢弃的工作区改动不在 ref 快照模型内；要可撤销需要「执行前自动 stash」式额外快照机制（超出本任务范围，建议另立任务）。日志保留（追溯用），preview 明示「不可自动撤销：可用 reflog / stash 保底」 |
| conflict resolution | **不可靠** | ❌ 明确不做（既有设计） | 文件内容（含用户编辑）不在 ref 快照内；恢复途径是 Abort 流程或手动编辑。日志保留并聚合（checklist 2），preview 明示「不可自动撤销」 |

## after_oid 回填时机方案（GF-16 checklist 3）

batch 任务经 T-05 队列异步执行，提交时 after 态未知。三种候选：

1. **任务完成回调**：需要改 `task/`（GF-10 刚落地的 ws-stash 任务类型所在，本轮禁止触碰）——否决。
2. **执行前预计算**：记录的是*推测值*（任务可能失败、并发可能改状态），撤销预览会展示不存在的"操作后态"——否决。
3. **轮询回填（采用）**：`batch_branch_op` 提交后经 `tauri::async_runtime::spawn` +
   `spawn_blocking`（F-43 合规：同步命令禁裸 `tokio::spawn`）派生轮询器，
   每 500ms 查 `TaskManager::get_status`，全部终态后按任务成功与否逐仓读当前
   ref 状态写回 `after_oid`（只填 NULL 行，不覆盖已知值；失败/取消的仓库保持
   NULL，撤销预览如实拒绝）。上限 15 分钟兜底。撤销执行时仍会重新校验实时
   状态，竞态窗口无害。

## 冲突日志合并方案（GF-16 checklist 2）

- 会话键（`core::conflict::session_key`）：`merge:<MERGE_HEAD oid>` /
  `pick:<CHERRY_PICK_HEAD>` / `revert:<REVERT_HEAD>` / `rebase:<original_head>`，
  一次操作的全过程稳定不变；无进行中操作（手工 stage 的冲突标记）回落旧行为
  （逐个文件一条）。
- 进程内会话注册表（`(repo_path, session_key) → log_id`，有界 Map）；
  merge_continue / merge_abort / rebase_continue / rebase_skip / rebase_abort /
  pick_continue / abort_pick 关闭会话——同一操作重跑（同 session key）会开新
  日志行，不会追加到旧行。App 重启则注册表清空，下一次 resolve 开新行（旧行
  保持原样）。
- 覆盖两条记录路径：命令路径（`commands/conflict.rs`）与 AI 提案的
  ConflictApply 任务路径（`task/worker.rs` 调 `record_operation_best_effort`
  的路由，无需改动 GF-10 文件）。摘要：`解决 N 个文件冲突：a、b…（≤5 个）等 N 个`。

## 验收标准

1. 新覆盖的操作在 OperationLogView 可 preview（看到将恢复到的状态）并 undo 成功。
2. 一次 10 文件冲突解决只产生 1 条操作日志。
3. 批量 branch op 的撤销预览显示前后两态。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；undo 纯函数有单测覆盖。

## 验证记录（2026-09-24）

- `GW_TEST_MANIFEST=1 cargo test --lib`（全量 1027 测试）：**1010 passed / 14 failed / 3 ignored**。
  14 个失败全部为既有环境依赖项：real_maven ×10（maven exited with code 1）、
  `process::pty::smoke_dead_session_reclaimed_from_table`（shell 未在时限内退出）、
  `node::workspace` ×2（Windows verbatim `\\?\` 路径前缀）、
  `pathutil::component_match_folds_case_on_insensitive_filesystems`（大小写敏感假设）。
  **基线对照**：`git archive HEAD src-tauri` 抽树重跑 `node::workspace / pathutil / process::pty`
  —— 同样 4 个 FAILED，证明与本任务改动无关；real_maven ×10 位于本任务零改动的
  `runtime/` 目录且与任务清单既有失败吻合。**本任务新增/修改测试 0 失败**。
- 新增单测（全过）：undo 闭环 ×6（cherry-pick 回退 + 进行中拒绝、merge abort 重跑 +
  状态漂移拒绝、stash drop/clear 精确恢复、worktree 重建 + 缺失分支拒绝、batch create
  删除 + tip 漂移拒绝、conflict_resolution/restore_files 不可撤销明示）、backfill 幂等、
  stash/worktree reflog 重建与 detached 恢复、merge 重跑冲突态、快照编解码、
  冲突会话合并（10 文件 = 1 条 + 关会话开新行 + 回落）、轮询终态判定。
- 前端：`pnpm build`（vue-tsc --noEmit + vite build）通过。
- `detect_changes()`：变更符号全部落在本任务文件 + GF-11 并行文件（graph/CommitGraph/
  BranchManager/GitGraph），受影响执行流为 Undo_operation 链（增量扩展）与 GF-11 前端流。
- 验收对照：① 新覆盖操作均有 preview→undo 闭环单测（见上）；②
  `commands::conflict::tests::ten_file_resolution_produces_one_log`：10 文件 = 1 条日志
  （10 个 item）；③ checkout/create 的 after_oid 由轮询器回填（`backfill_after_oids`
  只填 NULL 行），删除类保持 NULL 由前端渲染「已删除」，preview 展示完整前后态；
  ④ 全量测试通过（除上述既有环境失败）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：project-analysis:239/:194 未修清单 + 静态核验。待复现与修复。 |
| 2026-09-24 | 定位线索全部复验成立（batch.rs 行号轻微漂移至 140-142；worker.rs ConflictApply 路径同源刷屏）。impact 分析：`plan_item`/`run_undo`/`record_operation_best_effort` HIGH（中心匹配/记录函数，均为增量扩展不改存量臂），`preview_undo` MEDIUM，命令级包装 LOW。 |
| 2026-09-24 | 修复完成：白名单扩展 6 类（stash drop/clear、merge abort、cherry-pick、worktree remove、batch create + 不可撤销明示 conflict_resolution/restore_files）；冲突日志按会话合并（进程内会话注册表 + 完成路径关会话）；batch after_oid 轮询回填（`tauri::async_runtime::spawn` + `spawn_blocking`，F-43 合规）。新增单测：undo 闭环 ×6、会话合并（10 文件=1 条）、backfill 幂等、stash/worktree reflog 重建、merge 重跑、快照编解码、轮询终态判定。验证命令与结果见「验证记录」。 |
