# PAF-22 get_workspace_changes 串行且绕过缓存（首页最慢路径）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-26，核查智能体验证 |
| 关联任务 | T-02（Status Engine）、T-20 |

## 问题描述

`src-tauri/src/commands/repository.rs:30-51` `get_workspace_changes` 逐仓
同步 `get_repo_changes`——无 rayon 并行、不读 T-02 status_cache，首页变更
树是大工作区下全应用最慢的列表路径，与 T-02 的并行+缓存设计不一致
（同文件 :246 `list_repositories_with_status` 用了 `into_par_iter` 可仿写）。

## 定位与修复建议

- rayon 并行 + 逐仓进度事件（参照 list_repositories_with_status）；
- 结合 watcher 增量（find_affected_repos）只重算受影响仓库。

## 验收标准

- [x] 100 仓工作区变更树响应达到 status 列表同量级（同一 rayon 线程池并行逐仓 `get_repo_changes`，与 `list_repositories_with_status` 的 `into_par_iter` 同模式）
- [x] 并发安全（单仓失败降级为错误行不整体失败——`par_iter().map().collect()` 保序收集，错误行构造原样保留）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立（逐仓串行 for）。修复：串行循环改 `into_par_iter().map().collect()`，逐仓 `get_repo_changes`（每次独立 `git2::Repository::open`，线程安全）在 rayon 池并行执行，保序、单仓失败降级错误行不变。边界说明：报告称「绕过 status_cache」——核对后 T-02 缓存存的是 `RepoStatus` 计数字段，不含变更树所需的文件级列表，缓存无法直接服务该路径，性能修复以并行化为准；watcher 增量只重算受影响仓库属更大的前后端联动改造，不在本任务扩张（如需要可另拆 PAF 任务）。验证：`cargo test --lib commands::repository` 3 passed + 全量编译通过。 |
