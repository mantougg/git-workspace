# GF-15 批量 Pull 分叉后无策略跟进（dry-run 已能识别分叉，失败后断链）

> 状态：⬜ 未开始
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

## 修复范围 checklist

- [ ] 1. 分叉识别：从 dry-run / smart_pull 结果中聚合 diverged 仓库列表（本地 ahead+behind 同时非零）。
- [ ] 2. 批量跟进入口：预演面板或失败汇总处提供「对 N 个分叉仓库执行 merge / rebase」——选策略（merge / rebase / --ff-only 重试 / 跳过），逐仓确认或批量确认。
- [ ] 3. 分叉仓库执行后产生冲突的，自动进现有冲突队列（`smartMergeQueue` 模式，RepositoryList 已有）。
- [ ] 4. 遵守 Safety First（[../tasks/00-全局开发约束.md:13-18](../tasks/00-全局开发约束.md)）：批量 merge/rebase 前展示影响范围（每个仓库将并入/变基的提交数）。

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
