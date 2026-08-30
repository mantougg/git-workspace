# T-25 AI Commit Message / Commit Summary

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)、[T-08 错误处理 + 日志 + Secret Protection](./T-08-errors-logging-secrets.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · AI Git Assistant（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-04, T-08 |
| 对应 Roadmap | §22 AI Git Assistant、§23 AI Commit Message |

## 目标

基于 Git Diff 生成 Commit Message 与 Commit Summary，用户确认后提交，作为「AI as Assistant」的典型能力。

## 需求范围

- [x] 输入：当前变更的 Git Diff（工作区/暂存区）
- [x] 输出：规范 commit message（标题 + 正文要点）
- [x] Commit Summary：多仓库变更的批量摘要
- [x] 用户确认后再提交（不自动 commit）
- [x] 发送前 Secret 检测 + Preview + Exclude File/Directory（T-08 提供）
- [x] AI 结果按 diff hash 缓存复用，重复 review 不重复调用
- [x] 批量 review 前成本预估（发送字符数 / 仓库数）（Roadmap 评审增量）
- [x] 复用现有 OpenAI-compatible 调用链（`commands/ai.rs`），API Key 不落盘

## 架构 / 性能注意点

- 遵循 §24 原则：AI 只生成建议，提交动作始终由用户触发。
- diff 超长截断策略沿用现有 10k 字符逻辑，改为按 token 预算智能截断（保留文件结构概览）。
- 发送前 Preview 让用户可排除敏感文件，是硬要求。

## 验收标准

- [x] 输入 diff 输出规范 commit message，用户可编辑后提交
- [x] Commit Summary 覆盖多仓库变更概览
- [x] 发送前可见 Preview 并可排除文件/目录
- [x] 含 Secret 的 diff 被拦截或掩码
- [x] 相同 diff 命中 AI 结果缓存，不重复请求
- [x] 批量 review 前可见成本预估

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 开始开发 | 由 AI-08 承载 Commit Message / Commit Summary 场景实现。 |
| 2026-08-30 | 完成 | 由 AI-08 实现：CommitSuggestion 仅填入现有可编辑提交框；Commit Summary 走多仓库 Preview、Secret、排除与缓存链路。验证见 AI-08。 |

### 子任务清单

- [x] Commit Message 生成 prompt 与调用
- [x] Commit Summary 多仓库摘要
- [x] 确认后提交衔接（T-11）
- [x] Preview / 排除 / Secret 检测接入
- [x] AI 结果缓存与成本预估
