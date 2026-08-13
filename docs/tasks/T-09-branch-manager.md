# T-09 Branch Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-02 |
| 对应 Roadmap | §6 Branch Manager、§41 schema（branches / remote_branches / tags） |

## 目标

实现单仓库 Branch Manager：本地/远程分支与标签管理、ahead/behind 展示、分支间操作，作为 P0 Git Client 的基础能力。

## 需求范围

- [ ] 分支列表：Local Branches / Remote Branches / Tags 分区展示
- [ ] 当前分支高亮，`↑N`（本地领先）/ `↓N`（远程领先）展示（数据来自 T-02 本地 remote-tracking ref）
- [ ] 操作：Create Branch / Checkout / Delete / Rename / Merge / Rebase（入口）/ Compare / Push / Pull / Track Remote Branch / Set Upstream
- [ ] Compare：分支 A ↔ 分支 B 的 diff 与 commit 差集（复用 T-04/T-12）
- [ ] 危险操作（Delete / Merge / Rebase）走 §46 分级确认
- [ ] 分支数据落库 `branches` / `remote_branches` / `tags`（T-03 表结构）

## 架构 / 性能注意点

- 分支/标签列表走缓存（T-02），刷新不触发网络 fetch；ahead/behind 用本地 ref，Remote Branch 列表也是本地 `refs/remotes` 快照。
- 批量仓库场景下分支操作走统一任务队列（T-05），单仓库走即时命令。

## 验收标准

- [ ] 本地/远程/标签三类列表正确分组展示，当前分支高亮
- [ ] ahead/behind 数值与 `git status -sb` 一致，且不触发网络请求
- [ ] Create / Checkout / Rename / Delete / Set Upstream 全部可用且错误有结构化提示
- [ ] 分支 Compare 展示 commit 差集与文件 diff

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 分支/标签数据查询与落库
- [ ] 分支列表 UI（三区 + 高亮 + ahead/behind）
- [ ] 各分支操作 command 与确认流
- [ ] Branch Compare 视图
