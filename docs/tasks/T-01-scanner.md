# T-01 Scanner 硬化

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | 无 |
| 对应 Roadmap | §38 扫描器、§58 自动发现、§59 Ignore Rules |

## 目标

将现有 rayon 并行扫描器（`src-tauri/src/core/scanner.rs`）从「一次性全量扫描」硬化成可增量、可取消、可安全遍历的生产级扫描器。

## 需求范围

- [x] 增量扫描：以「路径 + `.git` 目录 mtime」为 key，已知且 mtime 未变的仓库跳过 `git2::Repository::open` 校验
- [x] 可取消：原子标志（`scan_cancellable`），扫描中途可中止且不泄漏线程
- [x] Symlink 保护：`follow_links(false)` 不下钻软链，避免循环 / 逃逸遍历
- [x] 忽略规则：Workspace 级 `.gitworkspaceignore`（目录名 + 相对路径前缀），叠加默认忽略目录
- [x] 三种触发：Refresh（`list_repositories` 复用缓存）/ Rescan（`scan_repositories` 增量全量）/ Scan Selected（`scan_repository_subtree` 指定子树）
- [x] 结果持久化：写入 `repositories` 表；被删除仓库由 `cleanup_stale_repositories` 软删除（`is_deleted` 标记失效，`upsert` 复活）
- [x] 暴露扫描进度事件（`scan_progress`）

## 架构 / 性能注意点

- **libgit2 `Repository` 不跨线程共享**：当前 `par_iter` 中每个线程独立 `git2::Repository::open` 后即丢弃是正确模式，必须保持；不要缓存 `Repository` 句柄跨线程复用（`git2::Repository` 非 `Send/Sync`）。
- 增量判断以「路径 + `.git` 目录 mtime」为 key，避免每次全量 `open` 校验。
- 扫描是 IO 密集，瓶颈在磁盘遍历而非 CPU，rayon 并行度不必拉满，避免与 status/其他任务争抢 IO。

## 验收标准

- [x] 100 仓库 workspace 二次扫描（全命中缓存）< 500ms（release benchmark 实测 34ms）
- [x] 扫描中点击取消，线程 < 1s 内停止且不 panic（scanner 层 `scan_cancellable` + `cancelled_scan_returns_empty` 测试；UI 取消按钮不在本任务「scanner 硬化」范围）
- [x] `.gitworkspaceignore` 声明的目录不被下钻
- [x] 断开的 symlink 目录不导致遍历报错或死循环
- [x] 仓库被移走后，Rescan 能正确标记其失效，UI 不再显示（软删除）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-13 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：.gitworkspaceignore 解析 + 可取消扫描（scan_cancellable）+ symlink 保护；`cargo test core::scanner` 4 passed。剩余：增量扫描、三种触发（Refresh/Rescan/Scan Selected）
| 2026-08-13 | ✅ | 完成剩余：增量扫描（路径+.git mtime key 跳过 open 校验）+ 三种触发 command（Refresh/Rescan/Scan Selected）+ 软删除（schema v2：is_deleted/git_dir_mtime）；`cargo test` 22 passed、release benchmark 100 仓库二次扫描 34ms、`npm run build` 通过

### 子任务清单

- [x] 设计增量扫描 key 与失效标记策略（路径 + `.git` mtime；失效由 is_deleted 软删除标记）
- [x] 实现取消机制
- [x] 实现 `.gitworkspaceignore` 解析
- [x] 实现 Refresh / Rescan / Scan Selected 三种触发
- [x] 编写单元测试（skip_dir、ignore、cancel、增量跳过/重校验、软删除/复活；symlink 由 follow_links(false) 保证）
