# GF-12 SmartMergeDialog 丢失冲突语义（baseOid 未用 / conflictType 硬编码）

> 状态：✅ 已完成
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

智能合并对话框（Smart Pull / merge 中断恢复用）有两个语义丢失缺陷：

1. `baseOid` prop 声明后**从未使用**——三路合并的 base 信息拿着却没用上。
2. `conflictType` 被**硬编码为 `"both-modified"`**——deleted-by-us / deleted-by-them /
   both-added 等冲突类型全部退化成同一种，图标分支（已写好）永远不触发。用户看不出
   「对方删除了文件」和「普通内容冲突」的区别，而这两者的正确处理方式完全不同。

## 修复结论（先验题：后端能否提供逐文件冲突类型？）

**能，且早已是文件级并已接通前端**——本次**无需改后端任何生产代码、无 IPC 变更**：

- `src-tauri/src/core/conflict.rs::conflict_files`（现 93-125 行）按 index
  stage 1/2/3 条目存在性判定四种形状：`both-modified` / `both-added` /
  `deleted-by-us` / `deleted-by-them`。
- `get_operation_state`（`src-tauri/src/commands/conflict.rs:17`）返回的
  `OperationState.conflicts: Vec<ConflictFile>` 已带每文件 `conflictType`，
  TS 侧 `src/types/conflict.ts::ConflictFile.conflictType` 早已注册。
- 对话框 `props.conflicts` 的来源 `SmartPullResult::Conflict.files` =
  `history::conflict_paths(&index)`（`core/history.rs:369`），与
  `conflict_files` 读同一 index 冲突集、同一 path 取值优先级
  （our → their → ancestor），**path 一一对应**。
- 结论：前端对话框打开时自行拉取一次 `get_operation_state` 即可拿到真实
  类型；`models/ipc_golden/` 无需重生成（未新增/变更字段，golden 测试与
  HEAD 基线一致失败集内的预存在项不受影响）。

**baseOid 语义核验**：`smart_pull`（`commands/git_ops.rs:405`）与
`MergeOutcome::Conflict.base_oid`（`core/merge.rs:60`）取的都是 **Merge 前
HEAD**，即 `merge_abort`（hard reset 回 HEAD，`core/merge.rs:179-187`）的
落点——**不是 merge base（共同祖先）**。因此 baseOid 用于 Abort 二次确认
展示回滚目标；真正的三路 BASE 内容由 `get_conflict_content` 的 `base`
字段提供（删除型冲突的三态预览已渲染它）。

## 定位线索（证据 + 2026-09-24 复现核验）

- ~~`src/components/git/SmartMergeDialog.vue:164`（baseOid prop 唯一定义处，零消费）~~
  ——复现成立。修复后 prop 仍在（L212），已有消费点：`abortTargetHint`
  （L556-563）拼入 Abort 二次确认内容。
- ~~`src/components/git/SmartMergeDialog.vue:184`（conflictType 硬编码 both-modified）~~
  ——复现成立。修复后 L258-263 从 `conflictTypeMap` 取真实类型，缺省回落
  `"both-modified"`（与修复前行为一致，保证零回归）。
- 死分支图标：修复前 268-276（typeIcon 按类型渲染，因硬编码永不触发）——
  复现成立。修复后 L380-407 新增 `typeLabel` / `typeTagType` / `iconClass`：
  侧栏字母图标按类型着色（删除=danger、双方新增=info、内容冲突=warning），
  原生 title 给中文标签；头部新增类型 n-tag。
- 删除型冲突的处理路径核验：git 对 delete/modify 冲突**不写冲突标记**，工作区
  只有存活侧内容——原逐 hunk 流程在此类文件上必然落入「该文件没有冲突标记」
  死角。修复后删除型改为整文件策略解决（`resolve_conflict`，所选侧缺席时
  后端 `core/conflict.rs:207-224` 移除文件），并渲染 BASE/OURS/THEIRS 三态预览。

## 修复范围 checklist

- [x] 1. 透传真实 `conflictType`（文件级，见「修复结论」）：前端按类型渲染图标
      （侧栏图标按类型着色 + 头部 n-tag）与推荐动作——`deleted-by-us` → 推荐
      采用 Theirs，`deleted-by-them` → 推荐 Ours（保留仍持有文件的一侧），推荐
      按钮 `type="primary"` 且带「（推荐）」；删除型无标记，按钮直达
      `resolve_conflict` 策略命令，逐 hunk 流程不适用。
- [x] 2. `baseOid`：**保留并使用**（二选一取「用」）——Abort 二次确认展示
      「回到操作前 HEAD <short 7>」（§46 危险操作影响范围）。未选「删 prop」的
      另一硬原因：两个调用方（`src/views/BranchManager.vue:398`、
      `src/views/RepositoryList.vue:842`）都在传该 prop，而 RepositoryList.vue
      是本次并行任务禁改文件。
- [x] 3. both-modified 行为零回归：hunk 解析/采用/应用代码路径原样未动，仅头部
      多一个类型标签；后端原 `detects_merge_conflict_state` 等既有用例全绿。

## 不做（范围控制）

- 不扩展新冲突操作（如「保留双方」的默认策略）。
- 不改后端冲突检测算法。

## 验收标准

1. 构造 delete-by-us / delete-by-them 冲突：解决器显示对应类型图标与正确的默认
   推荐动作。✅ 后端夹具单测 `deletion_conflict_types_are_distinguished`
   （类型判定）+ `deletion_conflict_resolution_matches_git_add`（四种
   our/theirs 组合的工作区文件状态 + `merge_continue`）全绿；前端图标/推荐随
   类型渲染（`pnpm build` 通过）。GUI 实机渲染由主智能体/用户后续验证。
2. both-modified 冲突行为与修复前一致。✅ 逻辑路径未动（见 checklist 3）。
3. `pnpm build` 通过；后端 `GW_TEST_MANIFEST=1 cargo test --lib` 与 pristine
   HEAD 基线失败集逐条一致（16 个预存在环境类失败：real_maven×10、
   real_node_vite、pty smoke、node workspace×2、pathutil 大小写、
   ts_types_match_rust_samples；本改动港湾零新增失败，新增 2 个测试全过）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（两处语义丢失，均已静态核验）。待复现与修复。 |
| 2026-09-24 | 修复完成：根因=对话框未消费后端已有的逐文件冲突类型（get_operation_state.conflicts[].conflictType，index stage 1/2/3 判定，path 与 props.conflicts 同源一一对应），且放弃了 baseOid。修法=①`loadConflictTypes()` 打开时拉取 operation_state 构建 path→类型映射（顺序合并不清空、换仓重置、失败回落 both-modified），侧栏图标/头部 tag 按类型渲染；删除型（deleted-by-us/them）无冲突标记，给出默认推荐（us→Theirs、them→Ours）并改为整文件 `resolve_conflict` 策略解决（删除侧由后端移除文件），同时渲染 BASE/OURS/THEIRS 三态预览替代「没有冲突标记」死角；②baseOid 用于 Abort 二次确认的回滚目标展示。后端零生产代码改动、零 IPC/golden 变更。验证：`pnpm build` ✅；`GW_TEST_MANIFEST=1 cargo test --lib` 在「HEAD+本改动」隔离树上 1014 passed / 16 failed，与 pristine HEAD 基线（1012 passed / 同 16 failed）失败集逐条一致；并行任务 GF-17 落盘后在真实树复跑 `cargo test --lib` = 1017 passed / 16 failed（ts_types_match 因 GF-17 golden 更新转绿、benchmark smoke 属已知环境抖动，均与本改动无关），`core::conflict` 7/7 全绿。注：并行任务在写 `src-tauri/src/commands|core/preview.rs`（GF-17 预演），其半成品期间整棵树无法编译，全量测试以隔离树（HEAD+本改动）结果为准。 |
