# AI-08 Commit / Review / PR / Explanation 场景

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-07](./AI-07-git-diff-pipeline.md)（公共 Diff 管道）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §14.2、§3.2 场景 C/D。本任务承载 [T-25](../tasks/T-25-ai-commit-message.md)（Commit Message / Summary）与 [T-27](../tasks/T-27-ai-pr-description.md)（PR Description / Security Review / Bug Detection / Explanation）的场景范围。

| 项 | 值 |
|---|---|
| 阶段 | Phase C · Git Assistant |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-07（覆盖 T-25/T-27 场景） |
| 对应设计文档 | §14.2 Commit Message、§8.4 结果模型、§4.2 Phase C |

## 目标

在公共 Diff 管道上交付四个 Git AI 场景：Commit Message / Commit Summary、Code Review / Security Review / Bug Detection、PR Description、Commit / File Explanation。差异只在任务 prompt 与输出 Schema。

## 需求范围

- [x] Commit Message（§14.2）：输入选定范围 diff + 文件状态 + 最近提交风格（可选）；输出 `CommitSuggestion { title, body[], type?, scope?, changedRepositories[], rationale }`；结果**可编辑**，最终提交仍走 T-11 流程与 T-08 安全检查，AI 不直接 Commit
- [x] Commit Summary / PR Description：从 Change Set 或 Workspace Dashboard 选多个 Repository 生成（§3.2 场景 C），允许排除 Repository/目录/文件
- [x] Code Review / Security Review / Bug Detection：输出 `ReviewReport { summary, issues[](file, line, severity, category, description) }`，兼容现有 `ReviewResult` 展示
- [x] Commit / File Explanation：解释指定提交或文件的历史与变更意图（只读）
- [x] 风险摘要：多仓库变更的风险点列表，标记 AI 推断
- [x] 各场景入口接入现有页面：Changes/Diff 视图、Change Set、History、Workspace Dashboard；不在视图内重复实现聊天状态（§12.3）
- [x] 全部场景复用 AI-07 管道：Preview、Secret、排除、缓存、结构化解析

## 架构 / 性能注意点

- 输出 Schema 进 golden 快照；`ReviewReport` 兼容现有 `ReviewResult` 类型（severity/category/file/description），UI 改动最小化。
- 多仓库 Summary 按仓库摘要优先，禁止逐行发送所有文件（§8.2）。
- Commit Message 结果进现有 Commit 面板的可编辑输入框，不新增并行提交路径。

## 验收标准

- [x] 四个场景均能产出结构化结果并正确解析/降级（§18.1 单测 + fake Provider 集成测试）
- [x] Commit 建议未确认时不提交（§18.2 测试断言）
- [x] 多仓库排除在 Preview 与结果中生效
- [x] AI 未配置/离线时各入口优雅降级，Git 核心功能不受影响
- [x] T-25 / T-27 spec 时间线追加「由 AI-08 实现」并更新状态（完成后同步两处 README）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 开始开发 | 恢复 AI-08：在 AI-07 公共 Diff/Preview/Gateway 管道上实现 Commit、Review、PR Description 与 Explanation 场景。 |
| 2026-08-30 | 完成 | 新增 Git 场景枚举、受信 prompt 与结构化结果 Schema；在 Changes/Diff、Change Set、Dashboard、History 接入统一 Preview 请求链。Commit 建议只回填现有可编辑提交框，提交解释通过 T-12 只读 Commit Diff 补充上下文。安全走查确认无 AI 直接 Git 写操作、进程 spawn 或凭证落盘。验证：`cargo test --lib ai::request::tests::git_scenarios_parse_to_their_structured_results --no-fail-fast`、`cargo test --lib ai::preview::tests::git_commit_scenario_uses_structured_schema_and_shared_preview --no-fail-fast`、`cargo test --lib ai::gateway_tests::git_scenario_uses_gateway_and_parses_structured_review --no-fail-fast`、`cargo test --lib ipc_golden --no-fail-fast`、`pnpm build`、`git diff --check`。 |

### 子任务清单

- [x] Commit Message / Summary 场景（prompt + Schema + 入口）
- [x] Review / Security / Bug Detection 场景
- [x] PR Description 场景
- [x] Commit / File Explanation 场景
- [x] 各页面入口与结果可编辑交互
- [x] 单元/集成测试 + T-25/T-27 状态同步
