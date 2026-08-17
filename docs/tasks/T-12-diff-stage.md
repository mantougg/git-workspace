# T-12 Diff 增强（Hunk / Line Stage + 多对象 Diff）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-04 |
| 对应 Roadmap | §9 Diff 增强 |

## 目标

将 Diff 从「只读查看」升级为「可交互暂存」：Hunk / Line 级 Stage/Unstage，并支持多种 Diff 对象（File/Hunk/Line/Commit/Branch/Tag/双点）。

## 需求范围

- [x] Diff 层级：File Diff / Hunk Diff / Line Diff（文件视图 + hunk 按钮 + 行级选择）
- [x] 对象间 Diff：Commit Diff / Branch Diff / Tag Diff / Commit A↔B / Branch A↔B（`get_commit_diff` + `get_revision_diff`，revparse 覆盖 branch/tag/oid）
- [x] Stage Hunk / Unstage Hunk / Stage Line / Unstage Line（libgit2 patch/apply：hunk 用 `ApplyOptions::hunk_callback` 过滤，行级/反向走单 hunk patch 重建 + `Diff::from_buffer`）
- [x] 与 T-11 联动：暂存的 hunk/line 参与 Commit（T-11 `index_only` 按 index 现状提交 + DiffViewer「提交暂存区」入口）
- [x] 保持 Unified 与 Side-by-Side 两种视图（暂存交互在 Unified 视图；Side-by-Side 只读）

## 架构 / 性能注意点

- line 级暂存依赖 libgit2 的 `Patch` 与 index stage 操作；line 数量大时按 hunk 分块处理。
- 双点 Diff（A↔B）复用 diff 缓存，key 用 `(old_oid, new_oid, path)`。
- 暂存状态变化要即时反映到状态缓存（T-02 失效对应仓库 status）。

## 验收标准

- [x] 单行 stage/unstage 后，工作区/暂存区状态与 `git diff --cached` 一致（`core::stage` 单元测试以 index blob 内容断言覆盖：行选暂存/取消、单删除行、EOF 无换行、stage→unstage 往返）
- [x] Commit/Branch/Tag/双点 Diff 四类对象均可用（`get_commit_diff` 含 root commit 空树兜底 + `get_revision_diff` 双 revspec；UI 比较栏 + GitGraph 提交详情「查看 Diff」入口）
- [x] 大文件 line 级操作不卡死、内存可控（暂存操作只重建单文件单 hunk 的 patch，IPC 仅携带行索引数组；视图侧 2000 行截断 + 虚拟滚动已在 T-04 兜底）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 T-11 联动闭环（index_only 提交）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发（T-04 剩余项「diff LRU 缓存」随本任务一起做） |
| 2026-08-14 | 🟦 | 核心完成：新增 `core/stage.rs`（stage/unstage hunk 用 apply hunk_callback 过滤；行级/unstage 走单 hunk patch 重建，reverse 先于行过滤，保留 `\ No newline` 语义）；`core/diff.rs` 拆出 unstaged（index→workdir）/staged（HEAD→index）diff + `diff_commit`（root 空树兜底）；revision/commit diff 进 AppState 有界 LRU（key=repo+old_oid+new_oid+flags，容量 32，T-04 联动）；`extract_hunks` 跳过 `\ No newline` 标记行使 UI 行索引与暂存索引对齐；前端 DiffViewer 加 未暂存/已暂存/比较 模式 + hunk 按钮 + 行选择（仅 Unified，Ignore 选项开启时禁用暂存），GitGraph 提交详情加「查看 Diff」入口；`cargo test` 87 passed（含缓存命中 1000 次 <50ms 微基准）、`pnpm build` 通过。剩余：T-11 联动（commit 按 index 现状提交） |
| 2026-08-14 | ✅ | T-11 联动闭环：`index_only` 按 index 现状提交（`index_only_commit_preserves_partial_staging` 验证 HEAD 只含已暂存行）+ DiffViewer「提交暂存区」入口（含安全预检/放行）；验收标准全部满足 |

### 子任务清单

- [x] Hunk / Line 级 Stage/Unstage 实现
- [x] 多对象 Diff 入口与查询
- [x] 暂存操作与状态缓存联动（apply 写 `.git/index` 触发 watcher 增量 status（T-02/T-06 链路），UI 操作后即时重载当前 diff）
- [x] 与 T-11 Commit 集成
