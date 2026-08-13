# T-01 Scanner 硬化

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | 无 |
| 对应 Roadmap | §38 扫描器、§58 自动发现、§59 Ignore Rules |

## 目标

将现有 rayon 并行扫描器（`src-tauri/src/core/scanner.rs`）从「一次性全量扫描」硬化成可增量、可取消、可安全遍历的生产级扫描器。

## 需求范围

- [ ] 增量扫描：对已发现且路径未变的仓库不重复校验，仅检测新增 / 删除的 `.git`（剩余：待 T-07 校准）
- [x] 可取消：原子标志（`scan_cancellable`），扫描中途可中止且不泄漏线程
- [x] Symlink 保护：`follow_links(false)` 不下钻软链，避免循环 / 逃逸遍历
- [x] 忽略规则：Workspace 级 `.gitworkspaceignore`（目录名 + 相对路径前缀），叠加默认忽略目录
- [ ] 三种触发：Refresh（复用缓存）/ Rescan（全量）/ Scan Selected（指定子树）（剩余：command 层）
- [x] 结果持久化：写入 `repositories` 表；被删除仓库由 `cleanup_stale_repositories` 清理（软删除剩余）
- [x] 暴露扫描进度事件（`scan_progress`）

## 架构 / 性能注意点

- **libgit2 `Repository` 不跨线程共享**：当前 `par_iter` 中每个线程独立 `git2::Repository::open` 后即丢弃是正确模式，必须保持；不要缓存 `Repository` 句柄跨线程复用（`git2::Repository` 非 `Send/Sync`）。
- 增量判断以「路径 + `.git` 目录 mtime」为 key，避免每次全量 `open` 校验。
- 扫描是 IO 密集，瓶颈在磁盘遍历而非 CPU，rayon 并行度不必拉满，避免与 status/其他任务争抢 IO。

## 验收标准

- [ ] 100 仓库 workspace 二次扫描（全命中缓存）< 500ms（以 T-07 Benchmark 实测为准）
- [ ] 扫描中点击取消，线程 < 1s 内停止且不 panic
- [ ] `.gitworkspaceignore` 声明的目录不被下钻
- [ ] 断开的 symlink 目录不导致遍历报错或死循环
- [ ] 仓库被移走后，Rescan 能正确标记其失效，UI 不再显示

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：.gitworkspaceignore 解析 + 可取消扫描（scan_cancellable）+ symlink 保护；`cargo test core::scanner` 4 passed。剩余：增量扫描、三种触发（Refresh/Rescan/Scan Selected）

### 子任务清单

- [ ] 设计增量扫描 key 与失效标记策略（剩余）
- [x] 实现取消机制
- [x] 实现 `.gitworkspaceignore` 解析
- [ ] 实现 Refresh / Rescan / Scan Selected 三种触发（剩余）
- [x] 编写单元测试（skip_dir、ignore、cancel；symlink 由 follow_links(false) 保证）
