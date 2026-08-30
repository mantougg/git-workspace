# AI-03 Context Builder / Preview / Secret / Token 预算

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-02](./AI-02-ai-gateway.md)（Gateway 与 ContextItem 契约）、[T-04](../tasks/T-04-diff-graph.md)（Diff）、[R-11](../tasks-runtime/R-11-log-engine.md)（日志）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §8、§10.1、§10.2。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-02, T-04, R-11 |
| 对应设计文档 | §8.1 上下文来源、§8.2 上下文预算、§8.3 Prompt 分层、§10.1 发送 Preview、§10.2 Secret 处理策略 |

## 目标

建立统一的上下文构建与发送前确认管道：从现有领域服务收集结构化上下文、按任务策略做预算与截断、生成 Context Manifest、Secret 扫描/脱敏/排除、输出 Preview 数据契约供前端确认。

## 需求范围

- [ ] Context Builder（§8.1）：只调用现有领域服务——Workspace/Repository store、Status Engine、T-04 Diff、History、T-16 Conflict、R-07 Runtime 配置、R-02/R-03 Closure、R-09~R-14 构建与错误、R-10/R-16 进程端口、R-11/R-13 日志、R-04/R-05 JDK/Maven；不直接扫描用户项目
- [ ] 上下文预算策略（§8.2）：尾部优先、结构优先、按任务分块——错误诊断（结构化错误 > 最近错误日志 > 日志尾部 > 环境摘要）、日志分析（选中范围 > 异常堆栈 > 前后少量上下文）、Code Review（文件清单与 hunk 结构 > 具体 diff）、Commit Message（变更文件/状态/摘要 > 完整 diff）、多仓库 Summary（每仓库摘要 > 逐行内容）
- [ ] token 估算（设计文档未定义方法，本任务定为实现细节）：以 chars/4 启发式为基准，按模型配置可校准系数；估算值进 `ContextItem.estimatedTokens`
- [ ] 预算超限处理：截断/排除项必须在 Manifest 与 UI 中可见，**不得静默强行发送**（§8.2）
- [ ] Secret 管道（§10.2）：复用 T-08 `scan_secrets`；`Block / Mask / Exclude / Warn` 四策略；**最终内容生成后扫描 + 脱敏后二次扫描**
- [ ] Preview 数据契约（§10.1 全字段）：Provider/模型、请求类型、目标 Workspace/Repository/Runtime、内容清单、每项字符数与估算 token、Secret 检测结果、自动脱敏项、被排除项、预计请求次数与可用时成本估算、是否使用网络
- [ ] 排除项变更 → 重新构建请求并重算 Secret 扫描、token 估算、内容 hash（§7.3）
- [ ] Prompt 分层（§8.3）：平台系统约束 / 角色约束 / 任务指令 / 结构化上下文（带来源标签）/ 输出 Schema；用户内容作为不可信数据显式标记，禁止字符串拼接进系统约束

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/{context.rs, policy.rs, redact.rs}`；`redact.rs` 只是 T-08 能力的统一调用适配，不另起扫描规则。
- 上下文收集全部异步，不阻塞 Runtime 状态机与日志采集；大日志/大 diff 分块处理（全局约束 §10）。
- Manifest 是审计与缓存 hash 的输入，字段必须稳定、可序列化、进 golden 快照。
- 前端 Preview Modal 组件落点 `src/components/ai/AiRequestPreview.vue`，用 tokens 变量，不硬编码样式。

## 验收标准

- [x] 五类预算策略各有单测：超限时的截断/排除顺序符合 §8.2
- [x] AI 请求前发现 AWS Key、JWT、私钥、密码、Token 时默认阻断（§18.2 集成测试）
- [x] 排除敏感文件后 Preview 内容不再包含被排除内容（测试断言）
- [x] Mask 后内容二次扫描仍命中时继续阻断（测试断言）
- [x] Prompt 分层单测：用户内容与系统约束隔离，来源标签齐全
- [x] Manifest / Preview 契约进 golden 快照

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发：Context Builder / 预算策略 / Secret 管道 / Preview 契约 / Prompt 分层 |
| 2026-08-30 | ✅ | 完成开发。落点：`ai/context.rs`（14 个领域服务收集器：Workspace store/Status Engine/T-04 diff 摘要+逐文件/History/T-16 冲突/R-07 redacted 配置/R-10 进程端口/R-11 日志 tail 与错误日志/R-04·R-05 环境/R-02·R-03 依赖/R-14 结构化错误；`ContextRole` 18 角色 + `TokenEstimator`（chars/4×校准系数）+ FNV-1a `content_hash`）、`ai/policy.rs`（五类预算策略 role→tier 映射 + 统一截断/排除执行，`MIN_TRUNCATE_TOKENS` 防无意义碎截）、`ai/redact.rs`（Block/Mask/Exclude/Warn，T-08 `scan_secrets`/`mask_secrets` 直用不另起规则，Mask 后二次扫描仍命中继续阻断；报告只含类别+计数，不含原文/位置）、`ai/prompt.rs`（§8.3 五层：平台约束常量/角色/任务指令/带来源标签+不可信声明的 user 消息/输出 Schema；用户内容绝不进 system）、`ai/preview.rs`（§10.1 全字段契约 + 零网络构建 + 无状态重建：排除项变更即重算扫描/估算/hash）、IPC `ai_build_context_preview`（spawn_blocking，不阻塞 Runtime 状态机/日志采集）、前端 `AiRequestPreview.vue`（tokens 变量）+ `api/ai.ts` + types。契约扩展（在时间线显式记录）：①`ContextItem` 增 `truncated`/`exclusionReason`（§8.2 可见性），`AiRequest` 增 `secretWarnConfirmed`（§10.2 Warn 需要 Gateway 侧放行依据；默认 false 保持全量阻断）——均进 golden 快照与 TS parity；②R-11 引擎补 `search_tail`（最近 n 行匹配）+ `RuntimeService::tail_logs/search_logs_tail`（AI-03「最近错误日志/日志尾部」需要 tail 语义，search 只返回最早 n 行）。安全走查：Preview/Gateway 全链零网络确认前不发请求（gateway 测试断言 call_count==0）；Secret 原文/位置不进报告与日志（仅类别+计数）；Runtime 配置只取 redacted 版；audit log 仅计量（task/provider/model/条目数/排除/截断/脱敏计数/est_tokens）；无 Command::new、无文件写入、不持有 Repository 句柄。验证：`cargo test --lib ai::` 97 passed；`cargo test --lib` 591 passed（`maven::settings` 2 个失败为本机 `~/.m2/settings.xml` 显式 localRepository 的既有环境性失败，R-11/R-10 同记录，与 AI-03 无关）；`GW_UPDATE_GOLDEN=1 cargo test --lib ipc_golden` 2 passed（快照已再生成，TS parity 通过）；`pnpm build`（vue-tsc + vite）通过；`detect_changes` 触点全部为预期变更（Gateway submit Warn 分支/lib.rs 注册/ContextItem 扩展/golden），风险等级 high 由触点数量放大，逐一核对无意外波及 |

### 子任务清单

- [x] Context Builder 与各领域服务接线和 Manifest 生成
- [x] 预算策略与 token 估算
- [x] Secret Block/Mask/Exclude/Warn 管道（含二次扫描）
- [x] Preview 数据契约 + `AiRequestPreview.vue`
- [x] Prompt 分层组装器
- [x] 单元/集成测试
