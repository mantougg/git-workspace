# AI-11 Action Proposal 与确认执行

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-05](./AI-05-tool-registry.md)（工具注册表/写工具占位）、[T-05](../tasks/T-05-task-queue.md)（Task Queue）、[T-24](../tasks/T-24-task-dag.md)（DAG）、[T-34](../tasks/T-34-undo-operation-log.md)（Undo/操作日志）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §10.3、§4.2 Phase E。

| 项 | 值 |
|---|---|
| 阶段 | Phase E · 受控写与外部 Agent |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-05, T-05, T-24, T-34 |
| 对应设计文档 | §10.3 写操作确认、§9.3 写工具占位、§12.1 IPC（`ai_approve_request` 与 Proposal 确认分离） |

## 目标

让 AI 从「只读建议」升级为「受控写操作」：对 Commit、Stage、Runtime Start/Stop、Apply Conflict 等动作生成**结构化提案**，统一展示影响范围与风险等级，用户确认后提交到现有命令/任务系统执行。模型不得直接调用任意系统命令。

## 需求范围

- [x] `ActionProposal` 结构（§10.3）：`proposalId / actionKind / riskLevel / targetScope / affectedRepositories / affectedFiles / beforeSummary / afterSummary / diff? / commandPreview? / reversible / expiresAt`
- [x] 写工具实现（接 AI-05 占位）：`git.createCommitProposal / runtime.startProposal / conflict.applyProposal / runtime.updateConfigProposal`——只生成 Proposal，**不直接修改领域状态**
- [x] `ai_proposals` 表启用（AI-04 已建）：状态流转 `pending → confirmed → executed / rejected / expired`，`executed_task_id` 关联 T-05 任务
- [x] 确认后执行路径：走现有命令注册表 / Task Queue / Command Safety / Operation Log（T-05/T-24/T-34）；Proposal 过期（`expiresAt`）后需重新生成
- [x] UI：`AiActionProposal.vue` 卡片——影响范围、风险等级、Diff、命令预览、可逆性、有效期；确认/拒绝/查看详情
- [x] IPC：`ai_approve_request`（批准发送请求）与 Proposal 确认命令**相互独立**（§12.1）
- [x] 风险等级展示约定：`low / medium / high`，high 风险（如 Runtime Stop、多仓库 Commit）需显式二次确认

## 架构 / 性能注意点

- Proposal 生成走统一调用链（Preview + Secret 仍然生效）；执行不再经过 AI 层，直接进任务系统。
- AI 层不持有 `Repository` 句柄、不 spawn 子进程（全局约束 §1）；执行结果经 Operation Log 可追溯，支持 Undo（T-34）。
- `commandPreview` 生成遵守平台规范（路径展示用原生分隔符，比较需归一化——根 AGENTS.md 平台规范 §1/§5）。

## 验收标准

- [x] Action Proposal 不会直接执行（§18.1 单测 + §18.4 走查：无直接 shell 执行路径、无直接改 Git/项目路径）
- [x] 确认后任务进入 Task Queue 并出现在 Operation Log；可 Undo
- [x] 未确认/已过期 Proposal 不产生任何副作用（§18.2 测试断言）
- [x] high 风险动作二次确认生效
- [x] `ai_proposals` 状态机单测（含 expired / rejected 分支）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31 完成开发与验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发：ActionProposal 状态机、受控写工具、Task Queue 确认执行与 Proposal UI |
| 2026-08-31 | ✅ | 完成：四个 Proposal 工具、SQLite 状态机、独立 IPC、Task Queue/Operation Log/Undo 管线及 UI；定向 Rust 测试、IPC golden、`cargo check`、`vue-tsc` 通过。全量 `cargo test --lib` 因环境 Java 集成测试长时间无响应而终止。 |

### 子任务清单

- [x] ActionProposal 类型与 `ai_proposals` 状态机
- [x] 四个写工具（仅生成 Proposal）
- [x] 确认执行管线（接 T-05/T-24/T-34）
- [x] `AiActionProposal.vue` 卡片与二次确认
- [x] 独立 IPC 命令 + golden 快照
- [x] 单元/集成测试
