# T-23 Workspace Pipeline

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-05 |
| 对应 Roadmap | §18 Workspace Pipeline、§21 任务依赖 DAG |

## 目标

实现把多个 Git 操作编排成任务流的 Pipeline：顺序/并行/条件/重试/超时/取消，并提供可视化编排与执行报告。

## 需求范围

- [ ] 步骤编排：Sequential / Parallel / Conditional / Retry / Timeout / Cancel
- [ ] 内置步骤：Fetch / Check Status / Pull / Build / Test / Report（可扩展）
- [ ] 示例流：Fetch All → Check Status → Pull Clean → Build → Test → Report
- [ ] 可视化编排器（步骤节点 + 连线）
- [ ] 执行报告：逐步结果、耗时、部分失败明细
- [ ] Pipeline 模板保存与复用

## 架构 / 性能注意点

- Pipeline 调度构建在 T-05 任务队列 + T-24 DAG 之上，步骤间依赖用 DAG 表达，执行并发遵守 §45 限流。
- Conditional 步骤的条件基于上一步结果（如「仅 pull 干净仓库」），结果数据流在内存传递，不落库中间态。

## 验收标准

- [ ] 示例 Pipeline 可端到端跑通并产出报告
- [ ] 条件步骤正确跳过不符合条件的仓库
- [ ] 步骤超时/失败可重试或跳过，不影响整体可控性
- [ ] Pipeline 可保存为模板复用

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Pipeline 模型与步骤定义
- [ ] 编排器 UI
- [ ] 调度执行（接 DAG）
- [ ] 执行报告与模板
