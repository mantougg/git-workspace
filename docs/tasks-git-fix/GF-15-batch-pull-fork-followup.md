# GF-15 批量 Pull 分叉后无策略跟进（dry-run 已能识别分叉，失败后断链）

> 状态：✅ 已完成（GUI 实机验证由主智能体/用户后续执行，前端逻辑与后端命令层均已构造数据验证）
> 优先级：P1（分析报告列为高价值建议：多仓库团队最痛的工作流断点）
> 来源：2026-09-24 Git 使用体验全景盘点；需求依据 [../project-analysis-2026-09-13.md:238](../project-analysis-2026-09-13.md)。

## 问题描述

批量 Pull（ff-only）遇到本地与远程分叉的仓库会失败。后端 `batch_dry_run` **已经能
在操作前预测哪些仓库会分叉**（rayon 并行 + `merge_commits` 内存预测冲突），但失败后
用户面对的是「N 个仓库失败」——没有一个「对这些分叉仓库批量选 merge / rebase 策略」
的跟进动作。多仓库团队日常最常卡住的正是这里：几十个仓库 pull 完，几个分叉的只能
逐个手动处理。

## 定位线索（证据）

- dry-run 实现：`src-tauri/src/commands/batch.rs:240`（`batch_dry_run`，rayon 并行 `:244`，`merge_commits` 预测 `:303`，含单测）
- 前端已消费预演：`src/views/RepositoryList.vue:519-543`（Pull 预演 / Push 预演按钮 + 弹窗）
- **现状备注（2026-09-24 核验）**：预演弹窗已有 `executeDryRun` 收口（`RepositoryList.vue:536-542`
  「对 N 个可快进仓库执行 Pull/Push」）——即**可快进仓库的跟进入口已存在**，本任务缺的
  正是**分叉仓库**那一半：预演/失败结果中 diverged 条目没有任何后续动作可选。
- smart_pull 结构化结果：`src-tauri/src/commands/git_ops.rs:226`（FF / merge / Conflict 三态）
- 现有可复用 UI：`src/components/git/SmartMergeDialog.vue`、`src/components/branch/RebaseDialog.vue`（后者已有 HTML5 拖拽先例 `:28-30`）
- 分析报告建议原文：project-analysis:238「dry-run 已能分类分叉仓库但缺『分叉→选 merge/rebase 策略』批量跟进」。

## 定位线索核验结论（2026-09-24 复现优先）

逐条复核（行号已漂移，按符号定位）：

1. **成立（漂移后仍成立）**：`batch_dry_run` 在 `src-tauri/src/commands/batch.rs:376-383`
   （rayon `par_iter` 在 `:381`），分叉预测 `dry_run_repo` 的 `merge_commits` 在 `:438-447`，
   含 `dry_run_categorizes_pull_and_push` 单测（`:505`）。GF-16 已在同文件追加
   after_oid 回填（`:243-353`），两者共存无冲突。dry-run 的 diverged 判定即
   `ahead>0 && behind>0`（`:436`），**识别能力已就绪**。
2. **成立（漂移后仍成立）**：RepositoryList 预演按钮 `:405-410`、弹窗 `:529-552`；
   `executeDryRun` 在 `:2238-2251`，`dryRunActionable`（`:2215-2217`）**只取
   `fast_forward`**——diverged / conflict 条目在弹窗里纯展示，无任何跟进动作。
   与文档「现状备注」完全一致。
3. **成立（漂移后仍成立）**：`smart_pull` 在 `src-tauri/src/commands/git_ops.rs:289-363`，
   `SmartPullResult`（`:31-46`）实为 UpToDate / Success(FF 或 merge commit) / Conflict
   三态（FF 与 merge 合并为 Success 一个变体）。
4. **成立**：`batch_pull`（`git_ops.rs:71-91`）→ 任务队列 `TaskType::Pull` → worker
   `pull_streaming` = `git pull --ff-only`（`task/worker.rs:523`、
   `core/git_ops/remote.rs:137-146`）：分叉仓库 fetch 成功但 FF 失败，任务记 Failed，
   用户只看到「N 个仓库失败」（GF-02 toast），**无分叉清单、无策略跟进**——问题复现。
5. **成立**：可复用设施确认——冲突队列 `smartMergeQueue`（RepositoryList `:1252`）+
   SmartMergeDialog（merge continue/abort）；rebase 核心 `core/rebase.rs::start_rebase`
   支持普通 rebase onto 任意 ref；merge 核心 `core/merge.rs::merge` 返回
   Merged / Conflict{files, base_oid}。分叉跟进所需积木全部存在，缺的只是串联。

**复现结论：问题成立**（后端识别能力就绪、前端 diverged 条目无后续动作、批量 pull
失败后断链），按修复范围实施。

## 修复范围 checklist

- [x] 1. 分叉识别：从 dry-run / smart_pull 结果中聚合 diverged 仓库列表（本地 ahead+behind 同时非零）。
- [x] 2. 批量跟进入口：预演面板或失败汇总处提供「对 N 个分叉仓库执行 merge / rebase」——选策略（merge / rebase / --ff-only 重试 / 跳过），逐仓确认或批量确认。
- [x] 3. 分叉仓库执行后产生冲突的，自动进现有冲突队列（`smartMergeQueue` 模式，RepositoryList 已有）。
- [x] 4. 遵守 Safety First（[../tasks/00-全局开发约束.md:13-18](../tasks/00-全局开发约束.md)）：批量 merge/rebase 前展示影响范围（每个仓库将并入/变基的提交数）。

## 实现说明（2026-09-24）

后端（`src-tauri/src/commands/batch.rs`，GF-16 追加区之后新增，未回退其改动）：

- 新命令 `batch_followup_diverged(repo_paths, strategy, op_id?)`（async，F-43 合规：
  async 宏入全局 runtime + `tauri::async_runtime::spawn_blocking` 跑阻塞 git 工作；
  经 `register_single_op` 登记批量级取消 flag，与 GF-07 单仓网络操作共用
  `cancel_git_op` 通道，逐仓间轮询取消、fetch 进行中即杀进程树）。
- 策略 `DivergedStrategy`：`merge`（`core::merge::merge` normal，分叉已消失时退化 FF）、
  `rebase`（`core::rebase::start_rebase` 默认 pick todo，**不做 interactive 批量版**）、
  `ff_only`（`git pull --ff-only` 重试）。
- 逐仓流程：fetch（流式镜像 Git Console，ConsoleStreamer 100ms 聚合，与 worker
  网络操作同底座）→ `upstream_divergence` 复判 ahead/behind → 策略落地。
  fetch 失败 = 该仓 failed（批次继续）；remote-tracking 过期时分叉判定以 fetch
  后为准（dry-run 快照可能过期）。
- 部分完成语义：`DivergedFollowupItem` 一仓一结果（merged / rebased /
  up_to_date / conflict / failed / skipped / cancelled）；conflict 携带
  conflictOp（merge|rebase）、files、baseOid 供前端冲突队列消费。
  rebase 成功仓按单仓 `start_rebase` 同规格落 T-34 操作日志（OP_REBASE，Undo 可回退）。
- 纯函数层（单测覆盖）：`classify_divergence`（NothingToPull / BehindOnly /
  Diverged）、`parse_strategy`、`FollowupSummary::of` 聚合。

前端（`src/views/RepositoryList.vue` + `src/api/batch.ts` + `src/types/batch.ts`）：

- 预演弹窗新增「分叉跟进」区（仅 Pull）：合并预览 `diverged` + `conflict` 两类
  （同属 ahead+behind 非零的分叉族）为候选清单，策略单选
  （Merge / Rebase / --ff-only 重试）→ 打开确认弹窗。
- 确认弹窗（Safety First）：影响范围表（仓库 / 本地 ahead / 将并入 behind /
  预演说明）+ 勾选（逐仓确认，取消勾选 = 跳过）+ 策略语义摘要
  （merge 共并入远程 N 个提交 / rebase 共重放本地 N 个提交）。
- 执行结果收口：merged/rebased/up_to_date 计数提示；conflict 进现有
  `smartMergeQueue`——merge 冲突走 SmartMergeDialog（既有路径），rebase 冲突走
  新增「Rebase 冲突处理」弹窗（在冲突解决器中处理 / Skip / Abort @§46 /
  已完成下一个；打开时先校验仓库是否仍在 rebase 冲突态，已被解决器处理完的
  自动出队）；failed/cancelled 走 GF-02 失败汇总 toast。
- 批量 pull（--ff-only）失败路径：`executeDryRun` 收口后对失败仓库重跑 dry-run，
  有分叉则自动弹出跟进确认（`offerDivergedFollowupAfterFailure`）。
- 非分叉仓库行为零变化：`dryRunActionable`/可快进执行路径未动；`handlePull` 的
  smartPull 循环仅给队列项补 `kind: "merge"` 字段（merge 分支行为不变）。

## 验证记录（2026-09-24）

- 新增单测（`cargo test --lib`，全过，真实 git 仓库夹具）：
  - `followup_merge_batch_partial_completion`：≥3 分叉仓 + 仅落后仓 + 无远程仓
    ——2 个干净合并（merge commit 双亲）+1 冲突（conflict/files=[conflict.txt]/
    baseOid/HEAD 未动/merge 进行中）+1 快进 +1 fetch 失败；汇总 3 merged/1 conflict/
    1 failed（部分完成语义）。
  - `followup_rebase_replays_local_commit_onto_upstream`：本地提交变基到
    origin/main（HEAD 父提交 = 上游 tip，rewritten=1），重跑为 up_to_date。
  - `followup_ff_only_refuses_diverged_repo`：仍分叉 → failed 且 detail 含
    `--ff-only`，HEAD 不动；仅落后仓重试成功。
  - `followup_pure_classification_and_summary`：分叉判定 / 策略解析 / 聚合纯函数。
- 全量：`GW_TEST_MANIFEST=1 cargo test --lib` → **1012 passed / 16 failed /
  3 ignored**。16 个失败与既有环境失败清单逐一对应（real_maven ×10 = JDK 版本
  错配「类文件主要版本 61.0 应为 52.0」、real_node_vite ×1、pty smoke ×1、
  node workspace ×2 与 pathutil 大小写 ×1 = Windows verbatim `\\?\` 前缀、
  ai gateway 重试超时 ×1），失败文件与本次改动零重叠；首轮发现的
  `models::ipc_golden::ts_types_match_rust_samples` 失败由本任务新增 TS 类型
  触发，已按模块规定注册 golden sample + `GW_UPDATE_GOLDEN=1` 重生成
  （diff 仅 +14 行 DivergedFollowupItem）后转绿。
- 前端：`pnpm build`（vue-tsc --noEmit + vite build）通过。
- `detect_changes()`：本任务符号增量（batch.rs 新命令/新结构体、lib.rs 命令
  注册、RepositoryList 冲突队列 kind 判别）；工作区 concurrent 变更
  （history.rs / GitGraph.vue / Reflog.vue，并行任务）另计，其 risk 偏高与本任务
  改动无因果关系。
- 验收对照：
  1. ≥3 分叉 + 正常仓：构造数据见 `followup_merge_batch_partial_completion`
     （2 分叉干净合并 + 1 分叉 rebase/ff-only 用例 + 仅落后仓）；预演面板与批量
     pull 失败路径均能进入一键批量 merge/rebase。GUI 实机待主智能体/用户验证。
  2. 冲突仓自动进 smartMergeQueue：测试断言 conflict 结果携带 files/baseOid；
     前端 `handleFollowupResults` 按 conflictOp 分流 merge/rebase 队列，
     其余仓库结果互不影响（部分完成语义已断言）。
  3. 非分叉仓库零变化：可快进路径（dryRunActionable/batchPull/batchPush）与
     handlePull 的 merge 行为未改；`GW_TEST_MANIFEST=1 cargo test --lib` 与
     `pnpm build` 通过（失败均为既有环境项）。

## 不做（范围控制）

- 不做 interactive rebase 的批量版（GF-20 规划项，本任务只做 merge/rebase 策略选择）。
- 不改 dry-run 算法本身。

## 验收标准

1. 构造 ≥3 个分叉仓库 + 若干正常仓库：批量 pull 后，预演/失败处能看到分叉清单，一键批量 merge（或 rebase）执行成功。
2. 其中 1 个仓库执行后冲突：自动进入冲突解决队列，其余仓库不受影响。
3. 非分叉仓库行为零变化；`cargo test --lib`（GW_TEST_MANIFEST=1）+ `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：分析报告高价值建议 + 后端 dry-run 能力已就绪（前端断链）。待复现与修复。 |
| 2026-09-24 | 开始修复（复现结论见「定位线索核验结论」）：问题成立——dry-run diverged 识别就绪，预演弹窗 `executeDryRun` 只覆盖 fast_forward；`batch_pull` 走 `git pull --ff-only`，分叉仓 fetch 后 FF 失败即断链。方案：batch.rs 新增 `batch_followup_diverged` 命令（fetch 流式镜像 + 逐仓 merge/rebase/ff-only 重试 + 执行时复判分叉），前端预演面板与失败路径加分叉清单 + 策略选择 + 影响范围确认，冲突入 smartMergeQueue。 |
| 2026-09-24 | 修复完成：根因 = 分叉识别能力就绪但「diverged 条目 → 策略执行」闭环缺失。修法 = 后端 `batch_followup_diverged`（async + spawn_blocking，F-43；单 op 取消通道；逐仓 fetch→复判→merge/rebase/ff-only；部分完成语义；rebase 成功落 T-34 日志）+ 前端预演分叉区/策略单选/影响范围确认弹窗（逐仓勾选可跳过）/rebase 冲突处理弹窗/批量 pull 失败后自动复判弹跟进。新增单测 ×4 全过；`GW_TEST_MANIFEST=1 cargo test --lib` 1012 passed / 16 failed（全部为既有环境失败，清单见「验证记录」；首轮 ipc_golden 漂移已按规注册 golden 后转绿）；`pnpm build` 通过。GUI 实机验证由主智能体/用户后续执行。 |
