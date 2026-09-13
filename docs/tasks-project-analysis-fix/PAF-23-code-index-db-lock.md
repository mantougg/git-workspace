# PAF-23 build_code_index 持全局 DB 锁贯穿扫描且无事务

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-27，主控亲自验证 |
| 关联任务 | T-28（符号索引/FTS5）、AI-05 |

## 问题描述

`src-tauri/src/commands/ai.rs` `build_code_index`（约 :898-1029）在
:931-937 一次取全局 DB 锁后贯穿整个 walkdir 目录扫描 + 逐条 INSERT——
大仓库索引期间所有 DB 使用方（AI Gateway、会话、状态刷新）被阻塞；且逐条
INSERT 无事务包裹，慢且中途崩溃留半索引（下次重建可自愈但浪费）。

## 定位与修复建议

- 扫描与落库分离：先无锁完成目录遍历收集条目，再持锁批量写入；
- 写入包事务分批提交；
- 评估索引期间允许只读查询（WAL 下读不阻塞写，但当前单写者 Mutex 会阻塞）。

## 验收标准

- [x] 索引大仓库期间其它 DB 操作不被长时间阻塞（锁只在每批 200 条的事务写入期间持有；walkdir 遍历与文件读取完全无锁）
- [x] 中途取消/失败不留半索引（或可快速自愈——重建开头 `DELETE FROM code_index WHERE repo_path` 全量清理旧条目，失败残留下次重建自愈；批内事务原子）
- [x] `cargo test --lib` 通过（commands::ai 3 passed，含批写持久化测试）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立（锁 :934-937 贯穿 walkdir + 逐条 INSERT）。修复：扫描与落库分离——walkdir 遍历、扩展名/大小过滤、文件读取全部无锁进行，条目攒批（`INDEX_BATCH = 200`，内存上界 ≈ 200 × 100KB 单文件上限 = 20MB）；批满或收尾调用 `flush_code_index_batch`（模块级私有函数）短锁内 `conn.transaction()` 批量 INSERT + commit。原有语义保留：开头 DELETE 全量旧条目（自愈路径）、batch_count 日志、单文件 100KB 上限与文本扩展名过滤。验证：`cargo test --lib commands::ai` 3 passed（新增 `code_index_batched_flush_persists_all_entries`：200/200/50 三批事务提交后 450 条全量可见）。 |
