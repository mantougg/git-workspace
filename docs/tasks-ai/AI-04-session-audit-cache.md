# AI-04 Session / Message / Request Audit / 结果缓存

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-02](./AI-02-ai-gateway.md)（请求生命周期）、[T-03](../tasks/T-03-sqlite-data-layer.md)（SQLite 数据层）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §10.4、§11。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | AI-02, T-03 |
| 对应设计文档 | §10.4 数据保留、§11.1 存储分层、§11.2 建议表、§11.3 缓存策略 |

## 目标

建立 AI 会话与请求审计的最小数据模型，以及按输入 hash 隔离的结果缓存，支撑后续 Assistant Drawer（AI-10）和各场景的结果复用。

## 需求范围

- [ ] 数据层迁移（§11.2）：`ai_sessions`（标题/角色/作用域/时间/归档）、`ai_messages`（session_id/role/content_json/sequence）、`ai_requests`（task_kind/provider_id/model_id/input_hash/context_manifest_json/status/error_code/token 用量/latency/时间戳）
- [ ] 会话 CRUD：创建、列表（分页）、读取（消息按需加载）、重命名、归档、删除；删除会话级联删除消息与相关本地缓存（§10.4）
- [ ] 会话持久化开关：用户设置控制是否保存完整会话；不保存时仅保留审计元数据（§10.4 / §11.1）
- [ ] 请求审计：记录状态迁移结果、上下文 manifest、内容 hash、Secret 计数与类别（不存原文）、token 用量（Provider 返回时）、耗时、错误 code
- [ ] 结果缓存（§11.3）：缓存 Key = `taskKind + modelId + promptVersion + contextHash + settingsHash`；内存 LRU（有上限）+ SQLite 持久；diff/日志/错误上下文/模型/Prompt 版本/脱敏排除策略变化必须失效
- [ ] 缓存结果标记生成时间与 context hash，供 UI 区分「过期结果」与「当前事实」
- [ ] `ai_proposals` 表结构预留（§11.2，AI-11 使用）：本任务只建表与类型，不实现 Proposal 流程

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/{session.rs, cache.rs}`；迁移遵守 T-03 的 WAL / 单写者 / 版本化约束。
- 默认**不保存**完整 Prompt 敏感原文；`ai_messages.content_json` 存结构化结果与展示所需内容，Secret 原文永不入库（全局约束 §6）。
- 缓存**不得跨模型/跨 Provider 复用**；读取时必须校验模型、Provider、Prompt 版本（§11.3）。
- 会话列表分页、消息按需加载，避免长会话拖慢 Drawer（§16.1）。

## 验收标准

- [ ] 缓存只在相同模型、Prompt 版本、context hash 下命中；任一维度变化即失效（§18.2 集成测试）
- [ ] 删除会话后无残留完整 Prompt 或 API Key（§18.2 集成测试）
- [ ] 审计记录不含 Secret 原文，只含计数与类别（测试断言）
- [ ] 迁移可重复执行、向后兼容，`ai_reviews` / `ai_tasks` 旧表不受影响
- [ ] 单元测试：会话 CRUD / 归档 / 分页、缓存 LRU 上限与隔离

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `ai_sessions` / `ai_messages` / `ai_requests` / `ai_proposals` 迁移
- [ ] 会话 CRUD 与持久化开关
- [ ] 请求审计写入（接 AI-02 生命周期）
- [ ] 结果缓存（LRU + SQLite、失效规则）
- [ ] 单元/集成测试
