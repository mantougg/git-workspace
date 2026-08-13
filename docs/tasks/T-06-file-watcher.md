# T-06 File Watcher 升级 + 事件聚合

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | T-02 |
| 对应 Roadmap | §39 File Watcher、§37 Status Engine、§43 IPC 事件 |

## 目标

将现有 `notify` PollWatcher（轮询 + 500ms debounce，`core/watcher.rs`）升级为 OS 原生监听，采用「每仓库一个 watcher」粒度，并补齐事件聚合与 IPC 节流，支撑大规模仓库场景。

## 需求范围

- [x] OS 原生监听：`RecommendedWatcher`（Windows ReadDirectoryChangesW / Linux inotify / macOS FSEvents），替换 PollWatcher
- [x] **每仓库一个 watcher**（每 repo 根 + `.git` 目录 NonRecursive），非全局单 watcher
- [ ] 仓库新增 / 删除时动态挂载 / 卸载（剩余：目前 `watch_repositories` 每次全量重建）
- [x] 500ms debounce 保持（`last_refresh` 去重，同仓库短窗口合并）
- [ ] IPC 节流：跨仓库 100ms 窗口批量推送（剩余）
- [x] 忽略 `.git` 与忽略目录：NonRecursive 只监听顶层，不递归大目录；status 刷新只读不自触发

## 架构 / 性能注意点

- **规模化关键**：1000 仓库 = 几十万~百万文件，全局单 watcher 会导致句柄 / 事件爆炸（Windows 尤甚）；必须按仓库粒度分片监听。
- 事件 → 增量状态链路（§37）必须闭合：watcher 只产出「受影响路径」，重算交给 T-02，不在 watcher 里做全量 scan。
- 批量操作完成时（如 500 仓库同时 finish）会瞬时产生大量事件，必须在发送端聚合，不能指望 UI 侧扛。

## 验收标准

- [ ] 500 仓库下监听不耗尽系统句柄 / inotify watch 上限
- [ ] 单次多文件变更只触发一次 status 重算（去重生效）
- [ ] 500 仓库同时变更，UI 事件流在批量聚合后仍流畅（无卡顿）
- [ ] 新增仓库后 watcher 自动挂载，删除后自动卸载

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：PollWatcher → RecommendedWatcher（OS 原生）；每仓库粒度 + 500ms debounce 已保留；`cargo check` 通过。剩余：增量挂载/卸载、IPC 批量窗口聚合

### 子任务清单

- [x] 评估并接入 OS 原生 watcher backend（RecommendedWatcher）
- [x] 实现每仓库 watcher 管理（每 repo + .git）
- [ ] 实现事件聚合与 IPC 批量推送（剩余）
- [ ] 与 T-02 联调增量链路（剩余：find_repo_root 与 find_affected_repos 统一）
- [ ] 大规模句柄压力测试（待 T-07）
