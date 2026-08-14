# T-09 Branch Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-02 |
| 对应 Roadmap | §6 Branch Manager、§41 schema（branches / remote_branches / tags） |

## 目标

实现单仓库 Branch Manager：本地/远程分支与标签管理、ahead/behind 展示、分支间操作，作为 P0 Git Client 的基础能力。

## 需求范围

- [x] 分支列表：Local Branches / Remote Branches / Tags 分区展示
- [x] 当前分支高亮，`↑N`（本地领先）/ `↓N`（远程领先）展示（数据来自 T-02 本地 remote-tracking ref）
- [x] 操作：Create Branch / Checkout / Delete / Rename / Merge / Rebase（入口）/ Compare / Push / Pull / Track Remote Branch / Set Upstream
- [x] Compare：分支 A ↔ 分支 B 的 diff 与 commit 差集（复用 T-04/T-12）
- [x] 危险操作（Delete / Merge / Rebase）走 §46 分级确认
- [x] 分支数据落库 `branches` / `remote_branches` / `tags`（T-03 表结构）

## 架构 / 性能注意点

- 分支/标签列表走缓存（T-02），刷新不触发网络 fetch；ahead/behind 用本地 ref，Remote Branch 列表也是本地 `refs/remotes` 快照。
- 批量仓库场景下分支操作走统一任务队列（T-05），单仓库走即时命令。

## 验收标准

- [x] 本地/远程/标签三类列表正确分组展示，当前分支高亮
- [x] ahead/behind 数值与 `git status -sb` 一致，且不触发网络请求
- [x] Create / Checkout / Rename / Delete / Set Upstream 全部可用且错误有结构化提示
- [x] 分支 Compare 展示 commit 差集与文件 diff

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-13 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 开始开发 |
| 2026-08-13 | ✅ | 完成：新增 `core/branch.rs`（三区列表 + upstream ahead/behind 纯本地 + create/checkout/delete/rename/set_upstream/track/compare）+ `diff::diff_revisions`（tree-to-tree diff，复用 extract_hunks 截断）+ `GitOps::push_branch`（git CLI，上游感知 refspec）+ `commands/branch.rs` 9 个 command + 落库 `replace_branches/remote_branches/tags`；前端 `BranchManager.vue`（三区 + 当前高亮 + ↑↓N + §46 确认流：Delete 二次确认/未合入强制删除升级/Push warning）+ Compare 对话框（commit 差集 + UnifiedDiff 复用）+ Merge/Rebase T-15 入口占位 + RepositoryList 分支入口；修复 checkout 顺序（先 checkout_tree 后 set_head，baseline 才是旧 HEAD）与同提交删除误判（graph_descendant_of 不含自身）；5 个核心单元测试；IPC golden 新增 5 类型登记；`cargo test` 49 passed、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] 分支/标签数据查询与落库
- [x] 分支列表 UI（三区 + 高亮 + ahead/behind）
- [x] 各分支操作 command 与确认流
- [x] Branch Compare 视图
