# T-24 Task DAG

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-05 |
| 对应 Roadmap | §21 任务依赖 DAG、§41 schema（tasks / task_items / task_dependencies） |

## 目标

把任务队列从「平铺队列」升级为「依赖 DAG」：任务间依赖、并行度、重试、取消、超时、部分失败，作为 Pipeline（T-23）的调度内核。

## 需求范围

- [x] 任务依赖：`task_dependencies` 表达 DAG，前置完成后触发后置
- [x] 并行度控制：按任务类型限流（§45），DAG 内可声明并行分支
- [x] 重试 / 取消 / 超时 / 部分失败（复用 T-05 状态机）
- [x] 依赖失败传播策略：fail-fast 或继续独立分支（可配置）
- [x] 可视化 DAG 展示（节点状态 + 依赖边）

## 架构 / 性能注意点

- DAG 调度器用拓扑排序 + 就绪队列，避免轮询；取消沿依赖边传播。
- 并发上限全局管控，DAG 分支并行不得突破 §45 的 Network/CPU 限流。
- 状态落库走 T-03 单写者，节点完成事件批量推送（复用 T-06 聚合思路）。

## 验收标准

- [x] 有依赖关系的任务按拓扑顺序执行，并行分支同时推进
- [x] 上游失败时下游按配置跳过或继续
- [x] 取消时整棵依赖子树停止且子进程清理
- [x] DAG 视图实时反映节点状态

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发：DAG 数据模型 + 拓扑调度器 + 失败/取消传播 + 可视化查询 |
| 2026-08-17 | ✅ | 完成：`task/dag.rs`（`DagState` 纯状态机：拓扑就绪队列 `initial_ready`/`release_dependents`、Continue 跳过子树 / FailFast 取消未完成节点、取消沿依赖边传播、条件跳过释放后继、调度级重试 `attempts<max_attempts`；`validate_edges` 入站前 Kahn 验环；`dispatch_ready`/`apply_outcome`/`cancel_pending_node`/`on_task_finished` 副作用助手接 manager/worker）+ manager `submit_dag`（合成 batch 行 + 节点任务 + `task_dependencies` 单事务落库 + 初始派发）/`cancel_dag`/`get_dag_graph` + worker 完成钩子（retried 节点跳过 batch 记账与清理）+ `commands/pipeline.rs` 的 submit_dag_tasks/get_dag_graph/cancel_dag；前端 PipelineView 步骤图（拓扑分层 SVG 连线 + 节点状态着色）实时反映运行状态（`task_progress` 聚合事件节流刷新 report）。验证：`task::dag::tests` 12 passed（修复 3 个测试：测试需模拟派发 `attempts=1` 以匹配「派发计数」语义）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败）；`vue-tsc --noEmit` 通过；IPC golden 补登记含 DagGraph/DagNodeInfo/DagEdge/DagSubmitRequest/DagNodeRequest/NodeCondition |

### 子任务清单

- [x] DAG 数据模型（task_dependencies）
- [x] 拓扑调度器 + 就绪队列
- [x] 失败传播与取消传播
- [x] DAG 可视化
