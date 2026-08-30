# AI-09 AI Conflict Resolution

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-07](./AI-07-git-diff-pipeline.md)（公共 Diff 管道）、[T-16](../tasks/T-16-conflict-resolver.md)（Conflict Resolver）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §14.3、§3.2 场景 E。本任务承载 [T-26](../tasks/T-26-ai-conflict-resolution.md) 的场景范围。

| 项 | 值 |
|---|---|
| 阶段 | Phase C · Git Assistant |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-07, T-16（覆盖 T-26 场景） |
| 对应设计文档 | §14.3 Conflict Resolution、§8.4 结果模型（ConflictProposal） |

## 目标

向 AI 发送冲突文件的 Base/Ours/Theirs 与必要项目上下文，返回建议内容与 Diff Preview；用户确认后才进入 T-16 的 Apply / Mark Resolved 流程。AI 不直接改工作区。

## 需求范围

- [x] 输入组装：Base/Ours/Theirs 三方内容 + 冲突文件路径 + 必要上下文（复用 AI-07 来源标记）；大文件**按 hunk 分批**，不发送整个 Repository（§14.3）
- [x] 输出 `ConflictProposal { proposedContent, diff, rationale, confidence }`（§8.4）
- [x] 应用流程固定为（§14.3）：`AI Suggestion → Diff Preview → User Confirmation → T-16 Apply → Mark Resolved`，任何一环不可跳过
- [x] 建议未确认时工作区零改动（测试断言）
- [x] 与 T-16 Conflict Resolver UI 集成：在冲突文件操作区增加「AI 建议」入口，建议结果以 Diff Preview 展示，确认后调用 T-16 现有 Apply 能力
- [x] 复用 AI-07 管道：Secret 扫描（冲突内容可能含配置/凭证）、Preview、缓存
- [x] 分批建议的进度与取消：大文件多 hunk 时可取消，已生成批次可查看

## 架构 / 性能注意点

- Apply 只调用 T-16 暴露的能力；AI 层不直接写冲突文件、不操作 index（全局约束 §1）。
- hunk 分批的上下文里必须带文件路径与 hunk 位置，避免 AI 建议错位；批次间无依赖时可并行请求（受 AI-02 并发上限约束）。
- 冲突内容常见凭证/配置，Secret `Warn` 策略需用户明确确认后才发送（全局约束 §5）。

## 验收标准

- [x] 典型冲突（双方改同一区块 / 一方删除一方修改 / 追加冲突）能给出可用建议与正确 Diff Preview
- [x] Conflict 建议未确认时不修改工作区（§18.2 集成测试）
- [x] 确认后进入 T-16 Apply / Mark Resolved，操作进操作日志（T-34）；冲突内容不保存，回退走 Git Abort 或手动重编辑
- [x] 大文件按 hunk 分批生效，且可取消
- [x] T-26 spec 时间线追加「由 AI-09 实现」并更新状态（完成后同步两处 README）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发：补齐冲突 hunk 上下文、结构化 ConflictProposal 与 T-16 Resolver 的 Preview/确认应用入口 |
| 2026-08-30 | ✅ | 完成：实现 hunk 分批建议、结构化 Proposal、Preview/确认后的 T-16 Apply 与操作日志；验证 `cargo test --manifest-path src-tauri/Cargo.toml ai::context::tests`、`cargo test --manifest-path src-tauri/Cargo.toml ai::request::tests`、`cargo test --manifest-path src-tauri/Cargo.toml core::conflict::tests`、`cargo test --manifest-path src-tauri/Cargo.toml commands::conflict::tests`、`cargo test --manifest-path src-tauri/Cargo.toml ipc_golden`、`pnpm build` |

### 子任务清单

- [x] Base/Ours/Theirs 上下文组装与 hunk 分批
- [x] ConflictProposal Schema 与解析
- [x] T-16 UI 集成（入口 + Diff Preview + 确认应用）
- [x] Secret/Preview/缓存接入
- [x] 单元/集成测试 + T-26 状态同步
