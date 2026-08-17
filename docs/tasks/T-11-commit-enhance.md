# T-11 Commit 增强

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-04 |
| 对应 Roadmap | §8 Commit 增强 |

## 目标

在现有「按仓库批量 Commit」基础上增强为完整 Commit 能力：Amend、部分提交（Selected/Hunk/Line）、Commit & Push。

## 需求范围

- [x] Commit / Amend / Commit --no-edit（`GitOps::commit` + `CommitOptions{amend, no_edit}`，git2 `Commit::amend`）
- [x] Commit Selected（勾选文件）（已有，本次接入安全检查预检）
- [x] Commit Hunk / Commit Line（依赖 T-12 的 hunk/line 暂存；`index_only` 按 index 现状提交，不再 `add_path` 整文件覆盖）
- [x] Commit & Push（`then_push` 提交后走 git CLI Push，进 T-05 任务队列；push 失败报「提交成功但推送失败」中间态，push 内部重试、worker 不重跑 commit）
- [x] Commit 前安全检查：Secret Scan + Large File Scan + Forbidden File Scan（`scan_commit` 预检 + `allow_unsafe` 放行，T-08 规则复用）
- [x] Commit UI：变更树勾选 → message → [提交]（Amend / 提交后 Push 选项 + 安全拦截放行对话框 + 提交身份对话框）；hunk/line 勾选走 T-12 DiffViewer →「提交暂存区」
- [x] Per-repo 提交身份：按仓库/分组配置 identity（schema v6 `author_name/author_email`，解析 repo > group > git 默认，提交时服务端自动选用）

## 架构 / 性能注意点

- hunk/line 级提交依赖 libgit2 的 patch/stage 能力；大文件 line 级操作要限制规模，避免内存爆炸。
- Commit 前安全检查是同步拦截，规则轻量、按文件 mtime 缓存（T-08）。
- 批量 Commit 走 T-05 任务队列并保持「按仓库分别提交」语义。

## 验收标准

- [x] Amend / --no-edit 语义正确（单测：amend 换 message+tree 且保持 parentage；--no-edit 保留原 message）
- [x] 只提交勾选的 hunk / line，未勾选部分保持未暂存（`index_only_commit_preserves_partial_staging`：HEAD 只含已暂存行，其余变更保持未暂存）
- [x] Commit & Push 失败时正确区分「提交成功但推送失败」的中间态（`commit_then_push_failure_keeps_commit_and_marks_state`：无效 remote 下 commit 保留 + 错误明确标注中间态；worker 对 commit 任务不重试，push 仅内部重试）
- [x] 含 `.env` / 大文件时被安全检查拦截并可放行（`safety_scan_blocks_and_override_allows` + `safety_scan_flags_large_files`；UI 预检对话框「仍要提交」放行）
- [x] 不同仓库/分组使用各自提交身份，切换无感（`commit_identity_resolution_prefers_repo_then_group`：repo > group 优先级与清除回退；`identity_override_is_used_for_commit`：提交作者/提交者生效）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 全部验收标准通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发（同时闭环 T-12 剩余项「commit 按 index 现状提交」） |
| 2026-08-14 | ✅ | 完成：`GitOps::commit` 重构为 `CommitOptions`（amend/no-edit/index_only/allow_unsafe/author override）；Commit & Push（then_push + push 内部重试 + 「提交成功但推送失败」中间态，worker 重试限定网络类任务）；安全检查升级为 secret/large/forbidden 三类 findings + `scan_commit` 预检 + 放行；schema v6 per-repo/group identity + 服务端解析注入；前端 Commit 面板（Amend/提交后 Push/身份对话框/拦截放行）+ DiffViewer「提交暂存区」（T-12 联动闭环）；TaskType::Commit 扩字段 + `rename_all_fields` 修复 + golden/TS 同步；`cargo test` 96 passed、`pnpm build` 通过 |

### 子任务清单

- [x] Amend / --no-edit 实现
- [x] 勾选文件提交（已有，补全）
- [x] Hunk / Line 级提交（依赖 T-12）
- [x] Commit & Push 与中间态处理
- [x] 接入 Commit 前安全检查
- [x] Per-repo 提交身份配置与选用
