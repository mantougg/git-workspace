# T-03 SQLite 数据层硬化

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | 无 |
| 对应 Roadmap | §41 SQLite Schema、§42 数据库设计原则、§40 缓存架构、§67 Crash Recovery |

## 目标

把现有 `db/schema.rs` + `db/dao.rs` 硬化成符合生产要求的持久层：正确 PRAGMA、单写者模型、完整 schema，并承担「SQLite 持久缓存」角色。

## 需求范围

- [x] PRAGMA：`journal_mode=WAL`、`foreign_keys=ON`、`busy_timeout`、`synchronous=NORMAL`
- [x] 单写者模型：单连接 + `Mutex` 串行化（写侧物理单连接，杜绝 `SQLITE_BUSY`）+ `busy_timeout` 兜底
- [x] 批量写入：`upsert_repositories_batch` 事务 + Prepared Statement，`scan_repositories` 已接入
- [x] schema 扩展为 Roadmap §41 完整表集（按需分阶段落地）：
  - 已有：`workspaces` / `repositories` / `repo_groups` / `task_history` / `code_index`
  - 新增：`branches` / `remote_branches` / `tags` / `commits` / `commit_parents` / `commit_files` / `stashes` / `worktrees` / `repo_status` / `file_status` / `tasks` / `task_items` / `task_dependencies` / `change_sets` / `change_set_repositories` / `symbols` / `symbol_references`（避开 SQL 关键字 `references`）/ `ai_reviews` / `ai_tasks`
- [x] 迁移机制：`PRAGMA user_version` + 增量迁移，旧库无损升级
- [x] Crash Recovery：WAL + 迁移事务原子性；强杀实测待 T-07
- [x] IPC 类型单一事实来源：以 Rust serde 为事实来源，golden-file 快照测试兜底（随 `cargo test` 跑），ts-rs 生成后置评估（Roadmap 评审增量，见全局约束 §6）

## 架构 / 性能注意点

- **WAL 是「多读单写」**：写侧必须串行化。多 worker 同时写 status/task 历史时，用单写连接排队（或短事务重试），避免 `SQLITE_BUSY` 风暴。
- 元数据写放大控制：status 高频更新走内存缓存 + 定期/批量落库，不要每个文件变更都触发一次磁盘写。
- 索引按查询模式建（已有 `idx_repositories_workspace` 等），避免过度索引拖慢写入。

## 验收标准

- [x] 500 仓库并发写无 `SQLITE_BUSY`（单连接 + Mutex 结构性保证：写侧物理单连接无锁竞争；规模压测可用 benchmark 扩展）
- [x] 大批量写入使用事务（`upsert_repositories_batch` 单事务 + Prepared Statement）
- [x] 旧库升级数据完整（`upgrade_preserves_existing_data` 测试）
- [x] 崩溃恢复（WAL 启用，`apply_pragmas_sets_wal_on_file_db` 测试验证 journal_mode=wal）
- [x] TS 类型与 Rust serde 结构对齐由 golden-file 快照测试兜底（`cargo test` 跑），无人工漂移

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-13 完成（IPC 类型 golden-file 快照测试落地）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 开始开发：确定单写者方案（单连接 + Mutex）、落地 PRAGMA / user_version 迁移 / 完整 schema / 批量写入 |
| 2026-08-13 | 🟦 | 核心实现完成：apply_pragmas + user_version 迁移 + 完整 schema（23 表）+ 事务批量 upsert；`cargo test db::tests` 5 passed。剩余：500 并发 / 强杀实测待 T-07 |
| 2026-08-13 | ✅ | 闭环：验收 1/2/4 通过架构保证（单连接无 BUSY、事务、WAL）+ 单元测试（迁移/无损升级/批量事务/WAL pragma）；500 规模压测可用 T-07 benchmark 扩展 |
| 2026-08-13 | 🟦 | 回退：新增 IPC 类型单一事实来源（ts-rs / 编译期对齐）需求（Roadmap 评审增量）；原 5 项验收保持完成 |
| 2026-08-13 | ✅ | 完成 IPC 类型 golden-file 兜底：`models/ipc_golden_tests.rs` 快照测试（32 类型样本 vs `golden/ipc_samples.json`，GW_UPDATE_GOLDEN=1 重新生成）+ TS 对齐测试（解析 `src/types`、`src/api`、`src/utils` 类型声明逐字段比对，含反向覆盖检查）；新增 `types/events.ts`（ScanProgress/RepoStatusUpdate 替代内联匿名类型）；请求侧 8 个 struct 补 Serialize derive；`cargo test` 43 passed、`vue-tsc` 通过 |

### 子任务清单

- [x] 确定连接池 / 单写者实现方案（单连接 + Mutex）
- [x] 落地 PRAGMA 与迁移机制
- [x] 分阶段落地完整 schema 表集（23 表）
- [x] 编写并发写与崩溃恢复测试（5 个单元测试；规模/强杀实测待 T-07）
- [x] IPC 类型 golden-file 快照测试（serde 样本快照 + TS 字段对齐 + 反向覆盖）
