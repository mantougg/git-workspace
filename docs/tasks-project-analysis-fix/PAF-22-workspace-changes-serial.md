# PAF-22 get_workspace_changes 串行且绕过缓存（首页最慢路径）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 100 仓工作区变更树响应达到 status 列表同量级
- [ ] 并发安全（单仓失败降级为错误行不整体失败，参照 change_set summary）
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
