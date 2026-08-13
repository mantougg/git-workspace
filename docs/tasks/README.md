# GitWorkspace 任务拆解总览

> 来源：`docs/GitWorkspace 产品需求与技术架构 Roadmap.md`（V1.0）
> 拆分原则：**按功能模块拆分**，每个任务一个独立文档（同目录下 `T-XX-<slug>.md`），可独立跟踪进度与验收。
> 本文件是唯一的总进度索引；每个任务文档内另有自己的「进度」章节。
>
> 贯穿所有任务的横切注意点（性能、libgit2 边界、操作安全、Secret、数据层、错误日志、Offline、文件监听）统一记录在 [00-全局开发约束.md](./00-全局开发约束.md)；各任务文档顶部标注了各自的最小加载集（全局约束 + 直接依赖）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**35**
- 已完成：**5** · 进行中：**3** · 未开始：**27**
- 完成度：**5 / 35（14%）**

---

## 阶段与任务索引

### Phase 0 · 基础稳定化（前置，8 个）

> 现有代码为早期原型，先硬化基础设施；Benchmark 提前到本阶段以校准全部性能目标。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| T-01 | Scanner 硬化 | P0 | ✅ | — | [T-01-scanner.md](./T-01-scanner.md) |
| T-02 | Status Engine（增量 + libgit2 线程安全 + 并发限流） | P0 | ✅ | T-01 | [T-02-status-engine.md](./T-02-status-engine.md) |
| T-03 | SQLite 数据层硬化（WAL / 单写者 / 完整 schema） | P0 | 🟦 | — | [T-03-sqlite-data-layer.md](./T-03-sqlite-data-layer.md) |
| T-04 | Diff & Graph 硬化 | P0 | 🟦 | T-03 | [T-04-diff-graph.md](./T-04-diff-graph.md) |
| T-05 | Task Queue 硬化（Partial Success / 重试 / 超时） | P0 | 🟦 | T-03 | [T-05-task-queue.md](./T-05-task-queue.md) |
| T-06 | File Watcher 升级 + 事件聚合 | P0 | ✅ | T-02 | [T-06-file-watcher.md](./T-06-file-watcher.md) |
| T-07 | Benchmark 系统（提前到 Phase 0） | P0 | ✅ | T-01, T-02 | [T-07-benchmark.md](./T-07-benchmark.md) |
| T-08 | 错误处理 + 日志 + Secret Protection | P0 | ✅ | — | [T-08-errors-logging-secrets.md](./T-08-errors-logging-secrets.md) |

### Phase 1 · 完整 Git Client（P0，9 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| T-09 | Branch Manager | P0 | ⬜ | T-02 | [T-09-branch-manager.md](./T-09-branch-manager.md) |
| T-10 | Stash | P0 | ⬜ | T-02 | [T-10-stash.md](./T-10-stash.md) |
| T-11 | Commit 增强（Amend / Selected / Hunk / Line / Commit+Push） | P0 | ⬜ | T-04 | [T-11-commit-enhance.md](./T-11-commit-enhance.md) |
| T-12 | Diff 增强（Hunk / Line Stage + 多对象 Diff） | P0 | ⬜ | T-04 | [T-12-diff-stage.md](./T-12-diff-stage.md) |
| T-13 | Cherry-pick / Revert / Reset | P0 | ⬜ | T-09 | [T-13-cherry-pick-revert-reset.md](./T-13-cherry-pick-revert-reset.md) |
| T-14 | Reflog | P0 | ⬜ | T-09 | [T-14-reflog.md](./T-14-reflog.md) |
| T-15 | Merge / Rebase | P0 | ⬜ | T-09 | [T-15-merge-rebase.md](./T-15-merge-rebase.md) |
| T-16 | Conflict Resolver | P0 | ⬜ | T-15, T-04 | [T-16-conflict-resolver.md](./T-16-conflict-resolver.md) |
| T-17 | Worktree | P1 | ⬜ | T-09 | [T-17-worktree.md](./T-17-worktree.md) |

### Phase 2 · Multi-Repo Engine（P1，9 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| T-18 | Workspace Dashboard | P1 | ⬜ | T-02, T-09 | [T-18-dashboard.md](./T-18-dashboard.md) |
| T-19 | Workspace Health | P1 | ⬜ | T-02 | [T-19-health.md](./T-19-health.md) |
| T-20 | Batch Operations 增强（选择器 + 批量操作全集） | P1 | ⬜ | T-05 | [T-20-batch-ops.md](./T-20-batch-ops.md) |
| T-21 | Workspace Stash & Branch | P1 | ⬜ | T-09, T-10 | [T-21-workspace-stash-branch.md](./T-21-workspace-stash-branch.md) |
| T-22 | Workspace Change Set | P1 | ⬜ | T-09, T-20 | [T-22-change-set.md](./T-22-change-set.md) |
| T-23 | Workspace Pipeline | P1 | ⬜ | T-05 | [T-23-pipeline.md](./T-23-pipeline.md) |
| T-24 | Task DAG（依赖 / 并行 / 部分失败） | P1 | ⬜ | T-05 | [T-24-task-dag.md](./T-24-task-dag.md) |
| T-33 | Workspace Manifest + 批量 Clone | P1 | ⬜ | T-01, T-05 | [T-33-workspace-manifest.md](./T-33-workspace-manifest.md) |
| T-34 | 统一 Undo / 操作日志 | P1 | ⬜ | T-05 | [T-34-undo-operation-log.md](./T-34-undo-operation-log.md) |

### Phase 3 · AI Git Assistant（P1，3 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| T-25 | AI Commit Message / Commit Summary | P1 | ⬜ | T-04, T-08 | [T-25-ai-commit-message.md](./T-25-ai-commit-message.md) |
| T-26 | AI Conflict Resolution | P1 | ⬜ | T-16, T-08 | [T-26-ai-conflict-resolution.md](./T-26-ai-conflict-resolution.md) |
| T-27 | AI PR Description + Security Review / Bug Detection / Commit Explanation | P1 | ⬜ | T-04, T-08 | [T-27-ai-pr-description.md](./T-27-ai-pr-description.md) |

### Phase 4/5/6 · Code Intelligence / Remote / Automation（P2，6 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| T-28 | Tree-sitter Symbol Index（定义 / 引用 / 调用层级） | P2 | ⬜ | T-03 | [T-28-symbol-index.md](./T-28-symbol-index.md) |
| T-29 | Remote Platform 集成 + Pull Request + CI | P2 | ⬜ | T-09, T-11 | [T-29-remote-platform.md](./T-29-remote-platform.md) |
| T-30 | Submodule / LFS / Hooks | P2 | ⬜ | T-02 | [T-30-submodule-lfs-hooks.md](./T-30-submodule-lfs-hooks.md) |
| T-31 | Command Palette + 快捷键 + IDE/Terminal 集成 | P2 | ⬜ | — | [T-31-command-palette.md](./T-31-command-palette.md) |
| T-32 | 插件系统 / Scheduled Tasks（Automation Platform） | P3 | ⬜ | T-23 | [T-32-plugin-system.md](./T-32-plugin-system.md) |
| T-35 | 发布工程（Updater / 崩溃上报 / 日志闭环 / 遥测） | P2 | ⬜ | — | [T-35-release-engineering.md](./T-35-release-engineering.md) |

---

## 关键依赖链

```text
T-01 Scanner ──► T-02 Status Engine ──► T-06 File Watcher
                     │                      │
                     │                      └──► 增量状态 → UI 事件
                     │
                     └──► T-09 Branch ──► T-13 / T-14 / T-15 / T-17
                                            │
T-04 Diff ──► T-11 Commit / T-12 Stage      └──► T-16 Conflict Resolver ──► T-26 AI Conflict
                                            │
T-03 SQLite ──► T-05 Task Queue ──► T-20 Batch / T-23 Pipeline / T-24 DAG / T-33 Manifest / T-34 Undo
                                            │
T-08 Secret Protection ──► T-25 / T-26 / T-27 AI 相关
                                            │
T-07 Benchmark（贯穿，校准所有性能目标）
```

- **Phase 0 全部完成后**才进入 P0 功能开发（基础设施不稳，上层功能返工成本高）。
- **T-07 Benchmark** 贯穿始终：每个性能相关验收标准都必须以真实 Benchmark 数据为准。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录。
3. 新增/调整任务时，重新编号并同步依赖字段。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
5. 全局横切约束统一记录在 `00-全局开发约束.md`；各任务文档的「架构/性能注意点」只写该任务特有内容，与全局约束叠加，不重复。
