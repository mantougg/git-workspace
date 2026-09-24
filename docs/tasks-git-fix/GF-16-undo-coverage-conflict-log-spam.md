# GF-16 Undo 覆盖面窄 + 冲突解决日志刷屏

> 状态：⬜ 未开始
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
- 覆盖清单（checkout/delete/reset/rebase/restore/conflict-resolution）：同文件白名单匹配，复现时定位具体枚举
- batch after_oid 恒 None：`src-tauri/src/commands/batch.rs:141`
- 冲突逐文件日志：`src-tauri/src/commands/conflict.rs:58-81`（每次 resolve 动作落一条）
- 前端消费：`src/views/OperationLogView.vue`
- 规范依据：Roadmap §46 危险三级 + 可恢复（`docs/GitWorkspace 产品需求与技术架构 Roadmap.md:1518-1566`）、`docs/tasks/00-全局开发约束.md:17`（Safety First）、`:36-40`（Reset/Rebase/删除前提示 reflog/stash 保底）

## 修复范围 checklist

- [ ] 1. 评估并扩展 undo 白名单：stash drop/clear、merge abort、cherry-pick、worktree remove、batch_restore 按「是否有可靠 ref 快照可回退」逐项评估（不可靠的明确不做并在文档记录原因）。
- [ ] 2. 冲突解决合并日志：一次 merge/rebase 的连续 resolve 合并为**一条**操作日志（含变更文件清单与前后 ref），不再逐文件刷屏。
- [ ] 3. batch_branch_op 记录 after_oid（批量 checkout/create/delete 后逐仓回填），撤销预览给完整前后态。
- [ ] 4. 撤销不可覆盖的操作（如 stash clear 若确定不回退）在日志中明示「不可撤销」，管理用户预期。

## 验收标准

1. 新覆盖的操作在 OperationLogView 可 preview（看到将恢复到的状态）并 undo 成功。
2. 一次 10 文件冲突解决只产生 1 条操作日志。
3. 批量 branch op 的撤销预览显示前后两态。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；undo 纯函数有单测覆盖。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：project-analysis:239/:194 未修清单 + 静态核验。待复现与修复。 |
