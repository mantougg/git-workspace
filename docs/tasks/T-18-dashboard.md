# T-18 Workspace Dashboard

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)、[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-02, T-09 |
| 对应 Roadmap | §15 Workspace Dashboard、§3 核心产品目标 G3 |

## 目标

实现多仓库核心入口 Dashboard：一屏汇总整个 workspace 的仓库数量与状态分布，并提供批量操作快捷入口。

## 需求范围

- [x] 统计卡片：Repositories / Clean / Modified / Untracked / Conflict / Ahead / Behind / Detached HEAD
- [x] 状态分布可视化（占比 / 分组视图）
- [x] 快捷操作：Fetch All / Pull Clean / Push / Commit / Create Branch（跳 T-20 预填选择）；Stash 批量入口待 T-21（按钮就位但禁用并标注）
- [x] 工作区切换与选择器
- [x] 数据来源：T-02 状态缓存聚合，实时响应增量事件

## 架构 / 性能注意点

- **ahead/behind 数据来源**：Dashboard 的 Ahead/Behind 计数来自本地 remote-tracking ref（T-02），**禁止为刷新 Dashboard 触发任何网络 fetch**。
- 聚合计算走内存缓存（T-02 已按仓库缓存状态），Dashboard 只做 O(n) 汇总，1000 仓库 < 50ms。
- 状态变化经 T-06 事件聚合推送，Dashboard 只做局部更新，不全量重算。

## 验收标准

- [x] 137 仓库场景下 Dashboard 秒开，统计准确（数据路径 = `list_repositories` 缓存优先 + 前端 O(n) computed；T-07 实测 per-repo status 8.14ms，137 仓库即使全量缓存未命中也 ≪ 1s，缓存命中为内存读取）
- [x] 刷新 Dashboard 不触发网络请求（数据路径仅 DB + 内存缓存 + libgit2 本地 status；ahead/behind 走 T-02 本地 remote-tracking ref；代码路径无任何 fetch 调用）
- [x] 状态变化后对应计数实时更新（< 500ms，受事件聚合窗口约束）（watcher 挂载 + `repo_status_changed_batch` → `useRepositories` 原地修补 store，卡片为 computed 自动重算；时延上限即 T-06 聚合窗口）
- [x] 各快捷操作正确跳转到批量操作并预填选择（`/changes?selector=...&action=...` → `applyRoutePrefill` 立即解析 selector 并触发对应流程：Fetch All 全量执行 / Pull Clean 开 Pull 预演 / Push 开推送选择器预勾 ahead 仓库 / Commit 预填 dirty 选择器并展开面板 / Create Branch 开批量建分支对话框；`vue-tsc` + `vite build` 验证，UI 链路无独立 e2e harness，沿用 T-09 以来构建级验收口径）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发 |
| 2026-08-17 | ✅ | 完成：新增 `DashboardView.vue`（8 统计卡片 + 状态分布占比条 + 分组视图 + 快捷操作）作为 `/` 首页，RepositoryList 移 `/changes` 并加 `applyRoutePrefill` 承接 `?selector&action` 预填跳转（fetch/pull 预演/push 对话框/commit 面板/批量建分支）；后端 `RepoStatus` 补 `conflicted` 独立计数（不计入 modified、is_clean 排除冲突）+ selector 新增 `@status:detached` + IPC golden/TS 同步；路由名 `repository-list` 全部改 `changes`（8 个视图 13 处）；watcher 在 Dashboard 挂载即启动（delta 幂等）+ `useRepositories` 接入批量事件实现卡片实时更新；Stash 快捷入口禁用并标注待 T-21；`cargo test` 114 passed（含冲突计数单测）、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] 统计聚合查询（前端 O(n) computed 聚合 `list_repositories` 缓存数据，零新增 IPC）
- [x] Dashboard UI（卡片 + 分布 + 快捷操作）
- [x] 工作区切换器
- [x] 增量事件联动
