# T-20 Batch Operations 增强

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-05 |
| 对应 Roadmap | §16 Workspace Batch Operations、§52 Workspace Selection |

## 目标

在现有批量 Add/Fetch/Pull/Push/Commit 基础上，扩展选择器（仓库/分组/标签/状态）与批量操作全集，形成核心差异化能力。

## 需求范围

- [ ] 选择器：Select Repositories / Groups / Tags / Status（如 `@group:frontend`、`@status:conflict`）
- [ ] 快速筛选：Dirty / Conflict / Ahead / Behind / Favorite
- [ ] 操作全集：Fetch All / Pull All / Push All / Commit All / Stash All（跳 T-21）/ Checkout All / Create Branch All / Delete Branch All
- [ ] 全部走 T-05 任务队列，逐仓库子结果、Partial Success、进度事件
- [ ] 危险批量操作（Delete Branch All）§46 分级确认，列出受影响仓库列表

## 架构 / 性能注意点

- 批量网络操作严格遵守 §45 并发限流（Fetch 8 / Pull 4 / Push 4），禁止按仓库数无上限 fork git 进程。
- 选择器过滤在内存缓存上做（T-02），不做 DB 全表扫描。
- 批量结果聚合展示（§20 任务面板样式：仓库级 ✓/✗ + 失败原因）。

## 验收标准

- [ ] 四种选择器组合过滤结果正确
- [ ] 100 仓库 Fetch All 并发被限制在 8，进程数可控（T-07 记录 Git Process Count）
- [ ] 部分失败正确标 Partial Success 且可定位失败仓库
- [ ] Delete Branch All 有危险确认并列出受影响仓库

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 选择器（group/tag/status）实现
- [ ] 快速筛选
- [ ] 批量操作全集接入任务队列
- [ ] 批量结果聚合 UI
- [ ] 危险批量确认流
