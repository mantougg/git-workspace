# T-26 AI Conflict Resolution

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-16 Conflict Resolver](./T-16-conflict-resolver.md)、[T-08 错误处理 + 日志 + Secret Protection](./T-08-errors-logging-secrets.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · AI Git Assistant（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-16, T-08 |
| 对应 Roadmap | §24 AI Conflict Resolution |

## 目标

基于 Base/Ours/Theirs + 项目上下文生成冲突解决建议，经预览与用户确认后应用，嵌入 T-16 Conflict Resolver。

## 需求范围

- [x] 输入：Base / Ours / Theirs / Project Context
- [x] 输出：Recommended Resolution
- [x] 流程闭环：AI Suggestion → Diff Preview → User Confirmation → Apply
- [x] 应用结果回写工作区并走 T-16 Mark Resolved
- [x] 发送前 Secret 检测 + Preview（T-08）

## 架构 / 性能注意点

- **硬约束（§24）**：AI 只给建议，必须经 Diff Preview + 用户确认后才 Apply，**禁止默认直接覆盖工作区**。
- 大冲突文件按 hunk 分批请求，避免超 token 预算；Project Context 用仓库元数据 + 相关文件摘要，不全量发送。

## 验收标准

- [x] 冲突文件可请求 AI 建议并预览解决前后 diff
- [x] 用户确认后 Apply 正确，未确认不修改工作区
- [x] 可回退（Git Abort / 手动重编辑；操作日志不保存冲突文本）
- [x] 含敏感信息的冲突内容发送前被检测

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | ✅ | 由 AI-09 实现：hunk 分批冲突建议、Secret/Preview、Diff Preview、用户确认后的 T-16 Apply / Mark Resolved 与操作日志。 |

### 子任务清单

- [x] 冲突上下文组装与 prompt
- [x] 建议生成与 Diff Preview
- [x] 确认 → Apply 流程
- [x] 与 T-16 集成
