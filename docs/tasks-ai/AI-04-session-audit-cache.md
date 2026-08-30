# AI-04 Session / Message / Request Audit / 结果缓存

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-02](./AI-02-ai-gateway.md)（请求生命周期）、[T-03](../tasks/T-03-sqlite-data-layer.md)（SQLite 数据层）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §10.4、§11。

| 项 | 值 |
|---|---|
| 阶段 | Phase A · AI Foundation |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-02, T-03 |
| 对应设计文档 | §10.4 数据保留、§11.1 存储分层、§11.2 建议表、§11.3 缓存策略 |

## 目标

建立 AI 会话与请求审计的最小数据模型，以及按输入 hash 隔离的结果缓存，支撑后续 Assistant Drawer（AI-10）和各场景的结果复用。

## 需求范围

- [x] 数据层迁移（§11.2）：`ai_sessions`（标题/角色/作用域/时间/归档）、`ai_messages`（session_id/role/content_json/sequence）、`ai_requests`（task_kind/provider_id/model_id/input_hash/context_manifest_json/status/error_code/token 用量/latency/时间戳）
- [x] 会话 CRUD：创建、列表（分页）、读取（消息按需加载）、重命名、归档、删除；删除会话级联删除消息与相关本地缓存（§10.4）
- [x] 会话持久化开关：用户设置控制是否保存完整会话；不保存时仅保留审计元数据（§10.4 / §11.1）
- [x] 请求审计：记录状态迁移结果、上下文 manifest、内容 hash、Secret 计数与类别（不存原文）、token 用量（Provider 返回时）、耗时、错误 code
- [x] 结果缓存（§11.3）：缓存 Key = `taskKind + modelId + promptVersion + contextHash + settingsHash`；内存 LRU（有上限）+ SQLite 持久；diff/日志/错误上下文/模型/Prompt 版本/脱敏排除策略变化必须失效
- [x] 缓存结果标记生成时间与 context hash，供 UI 区分「过期结果」与「当前事实」
- [x] `ai_proposals` 表结构预留（§11.2，AI-11 使用）：本任务只建表与类型，不实现 Proposal 流程

## 架构 / 性能注意点

- 代码落点（§5.2）：`src-tauri/src/ai/{session.rs, cache.rs}`；迁移遵守 T-03 的 WAL / 单写者 / 版本化约束。
  - **本任务新增 `src-tauri/src/ai/audit.rs`**：请求审计与会话/缓存是三种不同的生命周期（审计在请求进入与终态各写一次、会话由用户显式管理），与 `session.rs` 的单表 CRUD 混在一个文件会让「审计不含正文」这一硬约束难以走查；故按职责再拆一个模块，`session.rs` / `cache.rs` 落点不变。
- 默认**不保存**完整 Prompt 敏感原文；`ai_messages.content_json` 存结构化结果与展示所需内容，Secret 原文永不入库（全局约束 §6）。
- 缓存**不得跨模型/跨 Provider 复用**；读取时必须校验模型、Provider、Prompt 版本（§11.3）。
- 会话列表分页、消息按需加载，避免长会话拖慢 Drawer（§16.1）。

## 验收标准

- [x] 缓存只在相同模型、Prompt 版本、context hash 下命中；任一维度变化即失效（§18.2 集成测试）
- [x] 删除会话后无残留完整 Prompt 或 API Key（§18.2 集成测试）
- [x] 审计记录不含 Secret 原文，只含计数与类别（测试断言）
- [x] 迁移可重复执行、向后兼容，`ai_reviews` / `ai_tasks` 旧表不受影响
- [x] 单元测试：会话 CRUD / 归档 / 分页、缓存 LRU 上限与隔离

## 实现说明

- **缓存 Key 单一来源**：`cache::CacheKeyParts::for_request()` 是唯一组装入口（Prompt 版本取 `prompt::PROMPT_VERSION` 常量，context hash 与 settings hash 由 `request_content_hash()` / `settings_fingerprint()` 计算）。AI-03 的 Preview 复用同一个 `request_content_hash()`，保证「排除项变更 → hash 变更 → 缓存失效」在两条链路上一致。
- **settingsHash 覆盖脱敏/排除策略**：响应格式、温度、预算、工具策略、Warn 确认，以及 manifest 每条目的 `redacted / excluded / exclusionReason / truncated`（按 source_id 排序，与收集顺序无关）——满足 §11.3「Secret 脱敏/排除策略变化必须失效」。
- **缓存命中不联网**：Gateway 在 `Queued` 之后先查缓存，命中则走 `Sending → Parsing → Succeeded` 且零传输层调用（`AiRequestSnapshot.fromCache = true`，审计 status 记为 `cached`）。`AiRequest.useCache = false`（默认 true）用于 Drawer 的「重新生成」。
- **双层失效**：DB 侧靠 `ai_result_cache.session_id` 外键级联 + `session::delete_session` 内显式删除（不依赖 `PRAGMA foreign_keys` 实况），内存 LRU 由 `AiResultCache::invalidate_session()` 同步清理——否则会话已删仍会命中内存旧结果。
- **持久化开关**：`ai_settings` 的 `persistSessions`（默认关闭）。关闭时 Gateway 成功也**不写 `ai_messages`**，只留 `ai_requests` 审计（hash / manifest / Secret 计数 / token / 耗时）。
- **辅助设施不阻断主链路**：Gateway 的审计/缓存/会话写入全部经 `Gateway::with_db()`，失败只告警；`submit()` 在调用方持 DB 锁的上下文中执行，其 DB 写入一律走传入的 `&Connection`（`std::sync::Mutex` 不可重入）。
- **IPC**：`ai_create_session` / `ai_list_sessions` / `ai_get_session` / `ai_rename_session` / `ai_archive_session` / `ai_delete_session` / `ai_get_session_persistence` / `ai_set_session_persistence` / `ai_get_request_audit` / `ai_list_session_audits` / `ai_clear_result_cache`（设计文档 §12.1 的会话与审计命令）。UI 由 AI-10 承载。

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发：v15 迁移（sessions/messages/requests/cache/proposals/settings）+ 会话 CRUD + 审计接入 Gateway + 结果缓存 |
| 2026-08-30 | ✅ | 完成：会话/消息 CRUD 与按需加载、持久化开关、`audit.rs` 审计接入 Gateway 全生命周期（含 rejected/cached）、结果缓存（Key 隔离 + 内存 LRU + SQLite）、11 条 IPC 命令与 TS/golden 同步。验证：`cargo test --lib -- ai:: db:: models::ipc_golden`（148 passed）、`cargo check --all-targets`、`pnpm build`；golden 经 `GW_UPDATE_GOLDEN=1` 重新生成并核对 diff。安全类验收走查：审计无正文列（结构断言）、Secret 只记计数与类别、删除会话后全表扫描无 Prompt/Key 残留、持久化关闭时零消息落盘 |

### 子任务清单

- [x] `ai_sessions` / `ai_messages` / `ai_requests` / `ai_proposals` 迁移
- [x] 会话 CRUD 与持久化开关
- [x] 请求审计写入（接 AI-02 生命周期）
- [x] 结果缓存（LRU + SQLite、失效规则）
- [x] 单元/集成测试
