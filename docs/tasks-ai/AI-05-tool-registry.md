# AI-05 Tool Registry 与只读 Workspace/Runtime 工具

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-02](./AI-02-ai-gateway.md)（Gateway / toolPolicy）、[R-12](../tasks-runtime/R-12-ipc-task-integration.md)（IPC/Task 集成）、[R-13](../tasks-runtime/R-13-runtime-ui.md)（Runtime 数据 API）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §9.3、§9.4。

| 项 | 值 |
|---|---|
| 阶段 | Phase B · 工具注册表与 Runtime Assistant |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | AI-02, R-12, R-13 |
| 对应设计文档 | §9.3 工具注册表、§9.4 Agent 循环边界、§15 外部 Agent 能力规划 |

## 目标

把应用能力包装成**类型化、可审计、带权限矩阵**的只读工具注册表，作为应用内 Assistant（AI-06/AI-10）和未来外部 Agent Adapter（AI-12）的**唯一工具来源**。工具不是任意函数执行器。

## 需求范围

- [ ] 第一期 15 个只读工具（§9.3）：`workspace.list / repository.list / repository.status / repository.diff / repository.history / repository.conflicts / runtime.listApplications / runtime.getConfig / runtime.getProcessStatus / runtime.getClosure / runtime.getLogs / runtime.getErrorContext / jdk.list / maven.detect / task.getStatus`
- [ ] 每个工具的完整定义（§9.3）：稳定名称与版本、JSON Schema 输入、允许的角色、允许的上下文范围、是否需要当前 Workspace、是否可能包含 Secret、超时与结果大小上限、审计字段
- [ ] 角色-工具权限矩阵（§9.2）：Workspace Assistant / Git Reviewer / Commit Assistant / Conflict Assistant / Runtime Diagnostician / Runtime Config Advisor / Action Planner 各自白名单
- [ ] Agent 循环边界（§9.4）：单次用户请求工具调用上限默认 8 次，达上限返回「需要用户继续确认/缩小范围」；禁止自行扩大范围、后台观察、改用 shell、伪造工具结果
- [ ] 工具执行审计：每次调用记录工具名、参数 hash、耗时、结果大小、调用方角色（进 `ai.log`，不记敏感内容）
- [ ] 可能含 Secret 的工具结果（如 `runtime.getLogs`）必须经全局约束 §5 的扫描管道后才可进入上下文
- [ ] 写工具占位：仅定义命名约定（`git.createCommitProposal / runtime.startProposal / conflict.applyProposal / runtime.updateConfigProposal`），**不实现**（留待 AI-11）

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/tools.rs`；工具实现只编排现有领域服务（T-01/T-02/T-04、R-02/R-07/R-10/R-11/R-16 等），不复制领域数据、不新开 Git/Runtime 读取路径。
- 工具结果设数量与 payload 上限，超限截断并在结果中标记 `truncated`（§16.1）。
- JSON Schema 与 Rust 类型同源生成，进 golden 快照，供前端与外部 Adapter 复用。
- 工具调用全异步，带超时；不得阻塞 Runtime 主链路。

## 验收标准

- [ ] 权限矩阵单元测试：每个角色 × 每个工具的允许/拒绝符合 §9.2（§18.1）
- [ ] 全部工具只读：代码走查确认无写路径、无 shell 执行路径（§18.4）
- [ ] 工具结果超限截断并正确标记；超时返回结构化错误
- [ ] 单次请求工具调用达 8 次上限后停止并返回可行动提示（测试断言）
- [ ] 工具 Schema / 类型进 golden 快照，与 TypeScript 类型一致

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 工具定义框架（名称/版本/Schema/权限/上限/审计）
- [ ] 15 个只读工具实现
- [ ] 角色-工具权限矩阵
- [ ] Agent 循环上限与边界守卫
- [ ] 工具 Schema golden 快照
- [ ] 单元/集成测试
