# T-18 Workspace Dashboard

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)、[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-02, T-09 |
| 对应 Roadmap | §15 Workspace Dashboard、§3 核心产品目标 G3 |

## 目标

实现多仓库核心入口 Dashboard：一屏汇总整个 workspace 的仓库数量与状态分布，并提供批量操作快捷入口。

## 需求范围

- [ ] 统计卡片：Repositories / Clean / Modified / Untracked / Conflict / Ahead / Behind / Detached HEAD
- [ ] 状态分布可视化（占比 / 分组视图）
- [ ] 快捷操作：Fetch All / Pull Clean / Push / Commit / Stash / Create Branch（跳 T-20）
- [ ] 工作区切换与选择器
- [ ] 数据来源：T-02 状态缓存聚合，实时响应增量事件

## 架构 / 性能注意点

- **ahead/behind 数据来源**：Dashboard 的 Ahead/Behind 计数来自本地 remote-tracking ref（T-02），**禁止为刷新 Dashboard 触发任何网络 fetch**。
- 聚合计算走内存缓存（T-02 已按仓库缓存状态），Dashboard 只做 O(n) 汇总，1000 仓库 < 50ms。
- 状态变化经 T-06 事件聚合推送，Dashboard 只做局部更新，不全量重算。

## 验收标准

- [ ] 137 仓库场景下 Dashboard 秒开，统计准确
- [ ] 刷新 Dashboard 不触发网络请求
- [ ] 状态变化后对应计数实时更新（< 500ms，受事件聚合窗口约束）
- [ ] 各快捷操作正确跳转到批量操作并预填选择

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 统计聚合查询
- [ ] Dashboard UI（卡片 + 分布 + 快捷操作）
- [ ] 工作区切换器
- [ ] 增量事件联动
