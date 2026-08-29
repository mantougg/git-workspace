# AI-03 Context Builder / Preview / Secret / Token 预算

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-02](./AI-02-ai-gateway.md)（Gateway 与 ContextItem 契约）、[T-04](../tasks/T-04-diff-graph.md)（Diff）、[R-11](../tasks-runtime/R-11-log-engine.md)（日志）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §8、§10.1、§10.2。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
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

- [ ] 五类预算策略各有单测：超限时的截断/排除顺序符合 §8.2
- [ ] AI 请求前发现 AWS Key、JWT、私钥、密码、Token 时默认阻断（§18.2 集成测试）
- [ ] 排除敏感文件后 Preview 内容不再包含被排除内容（测试断言）
- [ ] Mask 后内容二次扫描仍命中时继续阻断（测试断言）
- [ ] Prompt 分层单测：用户内容与系统约束隔离，来源标签齐全
- [ ] Manifest / Preview 契约进 golden 快照

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Context Builder 与各领域服务接线和 Manifest 生成
- [ ] 预算策略与 token 估算
- [ ] Secret Block/Mask/Exclude/Warn 管道（含二次扫描）
- [ ] Preview 数据契约 + `AiRequestPreview.vue`
- [ ] Prompt 分层组装器
- [ ] 单元/集成测试
