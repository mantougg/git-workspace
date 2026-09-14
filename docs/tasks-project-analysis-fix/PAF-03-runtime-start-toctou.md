# PAF-03 Runtime 重复启动守卫 TOCTOU，并发产生双进程

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P0-3，主控亲自验证 |
| 关联任务 | R-10、R-15（多服务编排波内并行） |

## 问题描述

`runtime/launch/manager/start.rs:27-44`：`find_active` 检查（:27-37，一个
db 锁作用域）与 `insert_process`（:39-42，另一个锁作用域）分离，两次并发
Start 都可能在对方 insert 前通过检查 → 同一 (workspace, runtime) 双 spawn。
`db/schema.rs:610-634` 的 `runtime_processes` 表对活跃行无 UNIQUE 部分索引
（仅两个普通索引），DB 层无兜底；任务队列 8 worker 并发（lib.rs:170）使该
窗口真实可达。

## 定位与修复建议

- 把 check+insert 放入同一 db 锁临界区；
- 或加 SQLite 部分唯一索引兜底：
  `CREATE UNIQUE INDEX ... ON runtime_processes(workspace_id, runtime_name)
   WHERE status NOT IN ('stopped','failed')`（注意迁移与既有脏数据清理）。

## 验收标准

- [x] 并发双 Start 必有一个返回 Conflict，不再双进程
- [x] 端口预检之外的重复启动防护成立（R-15 波内并行不回归）
- [x] 并发单测通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 证据复核成立（`start()` 两个锁作用域仍分离）。修复：`find_active` 检查与 `insert_process` 收进**同一 DB 锁临界区**——manager 单连接 + Mutex 写序列化架构下即全量互斥，无需 SQLite 部分唯一索引（无跨进程写者，脏数据迁移成本不值）。回归测试 `concurrent_start_only_one_wins`：barrier 同步 8 线程并发 Start，断言恰 1 Ok + 7 Conflict。验证：`cargo test --lib` 881 passed |
