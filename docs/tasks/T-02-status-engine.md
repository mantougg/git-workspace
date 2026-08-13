# T-02 Status Engine

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-01 Scanner](./T-01-scanner.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | T-01 |
| 对应 Roadmap | §37 Status Engine、§40 缓存架构、§45 并发策略、§35.1 总体架构 |

## 目标

把现有 `core/git_status.rs` 重构成独立的 Status Engine：增量刷新、并发限流、内存缓存，并明确 libgit2 线程安全边界与 ahead/behind 的数据来源。

## 需求范围

- [x] 增量状态：`find_affected_repos`（路径 → 受影响仓库映射），只重算受影响仓库
- [x] 并发限流：status 计算改 rayon 并行（受线程池上限约束，§45 status ~16）
- [x] 内存缓存：`status_cache`（DashMap）已缓存 status；LRU 上限剩余（1000 仓库实际内存小，防御性）
- [x] ahead/behind：基于本地 remote-tracking ref（`upstream()` + `graph_ahead_behind`），**不触发网络 fetch**
- [x] 状态分级事件：`scan_progress`（已有）；`repository_status_changed` 待 T-06 watcher
- [x] 状态分类：clean / modified / untracked / conflict / ahead / behind / detached head（`RepoStatus`）

## 架构 / 性能注意点

- **libgit2 线程安全（本任务最关键约束）**：`git2::Repository` 非 `Send/Sync`。并行计算 status 时，每个任务独立 `Repository::open` / `drop`，禁止把 `Repository` 句柄放进内存缓存供其他线程复用。缓存里只放**纯数据**（status 结果、commit oid、ahead/behind 数值），不放 libgit2 句柄。
- **ahead/behind 数据来源**：用本地 `refs/remotes/origin/*` 与本地分支比较；缺失 remote-tracking ref 时显示「未知」而非拉取。这是正确性与性能的双重关键，杜绝打开/刷新即网络 fetch。
- 缓存分层：Memory（LRU，热数据）→ SQLite（持久元数据）→ Git 仓库（冷数据按需打开）。

## 验收标准

- [ ] 单仓库状态刷新 < 100ms，普通增量刷新 < 300ms（T-07 实测）
- [ ] 1000 仓库空闲内存 < 500MB（LRU 上限生效）
- [ ] 并发 status 时无 libgit2 跨线程句柄复用（代码评审确认 + 无 UB）
- [ ] 打开 Dashboard/刷新列表不触发任何网络 fetch
- [ ] 变更 10 个文件只重算受影响的 N 个仓库，而非全 workspace

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：status 计算 rayon 并行（并发限流）+ find_affected_repos 增量定位；ahead/behind 确认已本地化；`cargo test` 18 passed。剩余：LRU 缓存上限、与 T-06 联调

### 子任务清单

- [x] 设计 status 缓存 key 与失效策略（path key；LRU 上限剩余）
- [x] 实现受限并发线程池（rayon 并行）
- [x] 实现增量定位（路径 → 仓库映射）
- [x] 实现 ahead/behind 本地计算（确认已有实现正确）
- [ ] 与 T-06 File Watcher 联调增量链路（待 T-06）
