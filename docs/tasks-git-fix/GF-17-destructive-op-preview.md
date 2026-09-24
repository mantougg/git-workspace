# GF-17 破坏性单仓操作无结构化预演（merge/rebase/reset 仅文案确认）

> 状态：⬜ 未开始
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

## 修复范围 checklist

- [ ] 1. 后端 preview 命令（纯本地、只读）：`preview_reset`（将丢弃的提交清单 + 工作区变更清单）、`preview_merge`（将并入的提交 + 受影响文件 + 冲突预判）。
- [ ] 2. rebase 预演评估：自研 rebase 状态机（`core/rebase.rs`）可列出将变基的 commit 序列与每步冲突概率；若成本高则先做 reset/merge 两个（实现时定并记录）。
- [ ] 3. 前端确认框改造：从纯文案升级为「将发生什么」结构展示（提交数/文件数/不可恢复项标红），沿用现有 confirm 组件体系。

## 不做（范围控制）

- 不做「预演后可编辑策略」（如预演里改 merge flag）。
- 不改 batch_dry_run 本身（GF-15 消费）。

## 验收标准

1. reset --hard 前：预览列出将被丢弃的提交（oid+message）与文件变更清单。
2. merge 前：预览列出将并入提交数、受影响文件数、是否预判冲突。
3. 预演命令不改仓库任何状态（单测断言 ref 不变）；`cargo test --lib`（GW_TEST_MANIFEST=1）+ `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：dry-run 模式向单仓破坏性操作扩展（静态核验 + 规范依据 Roadmap §46）。待复现与修复。 |
