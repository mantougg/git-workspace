# T-23 Workspace Pipeline

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | 🟦 进行中 |
| 依赖 | T-05 |
| 对应 Roadmap | §18 Workspace Pipeline、§21 任务依赖 DAG |

## 目标

实现把多个 Git 操作编排成任务流的 Pipeline：顺序/并行/条件/重试/超时/取消，并提供可视化编排与执行报告。

## 需求范围

- [x] 步骤编排：Sequential / Parallel / Conditional / Retry / Timeout / Cancel
- [x] 内置步骤：Fetch / Check Status / Pull / Build / Test / Report（可扩展）
- [x] 示例流：Fetch All → Check Status → Pull Clean → Build → Test → Report
- [x] 可视化编排器（步骤节点 + 连线）
- [x] 执行报告：逐步结果、耗时、部分失败明细
- [x] Pipeline 模板保存与复用

## 架构 / 性能注意点

- Pipeline 调度构建在 T-05 任务队列 + T-24 DAG 之上，步骤间依赖用 DAG 表达，执行并发遵守 §45 限流。
- Conditional 步骤的条件基于上一步结果（如「仅 pull 干净仓库」），结果数据流在内存传递，不落库中间态。

## 验收标准

- [x] 示例 Pipeline 可端到端跑通并产出报告
- [x] 条件步骤正确跳过不符合条件的仓库
- [x] 步骤超时/失败可重试或跳过，不影响整体可控性
- [x] Pipeline 可保存为模板复用

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发：Pipeline 模型 + 编译到 DAG + 编排器/报告 UI（与 T-24 并行推进） |
| 2026-08-17 | ✅ | 完成：`core/pipeline.rs`（`Pipeline`/`PipelineStep`/`StepKind` 模型 + `validate_pipeline`（id 唯一/依赖存在/命令非空/不可依赖 Report/环检测）+ `compile_pipeline` 编译为「每 (步骤×仓库) 节点、逐仓库链」的 T-24 DAG（显式 depends_on 并行分支、条件/重试/超时映射到节点）+ `sample_pipeline` 示例流 + JSON 模板存储 + `build_run_report` 执行报告（逐步聚合/耗时/部分失败明细，Report 为虚拟汇聚步骤））+ `commands/pipeline.rs` 9 command（模板 CRUD/get_sample/run_pipeline/get_pipeline_run，编译后走 `submit_dag`）；前端 types/api + PipelineView（步骤编辑器：类型/命令/条件/重试/超时/依赖选择 + 拓扑分层 SVG 流程图连线 + 模板加载/保存/删除 + 运行/取消 + 执行报告逐步明细与耗时）；Check Status 步骤编译为 `git status --porcelain` ShellCommand，条件经 libgit2 内存求值。验证：`core::pipeline` 6 tests passed（示例流编译/校验拒绝/并行分支/retry 映射/模板往返/report 聚合）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败）；`vue-tsc --noEmit` 通过；IPC golden 补登记 12 个类型 |

### 子任务清单

- [x] Pipeline 模型与步骤定义
- [x] 编排器 UI
- [x] 调度执行（接 DAG）
- [x] 执行报告与模板
