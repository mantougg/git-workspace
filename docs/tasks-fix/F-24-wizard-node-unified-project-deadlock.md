# F-24 新建应用切「前端工程」应用永久无响应（RuntimeService 自死锁）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 关联 | N-09（统一项目视图引入）、b4d7fd1（部分修复未及根因） |

## 问题描述

新建 Runtime 应用时，把「运行时类型」从 Spring Boot 切到「前端工程」，页面自动刷新（node 项目加载），随后**整个应用永久无响应**——任何操作都卡死。

复现案例：新建应用向导 → 点击「前端工程」radio → 应用冻结。

## 定位线索

- 切换类型触发 `RuntimeAppWizard.vue onKindChange → loadNodeProjects() → runtime_list_unified_projects`。
- b4d7fd1 已把文件系统扫描移出 DB 锁，但**嵌套锁仍在**，故问题未消。
- `AppState.db` 与 `RuntimeService.self.db` 是同一个 `Arc<Mutex<Connection>>`（lib.rs 两处 `Arc::clone(&db)`），`std::sync::Mutex` 不可重入。

## 修复范围

- [x] `runtime_list_unified_projects`：锁内不再调 `state.runtime.list_projects()`（内部 `self.db.lock()` 二次加锁 = 自死锁），改用已持有的 `conn` 直接 `query_dependency_graph`；锁外扫描 + 二次加锁写索引的结构保留。
- [x] 全仓 brace-aware 扫描确认无其他命令在持有 `state.db` 作用域内调会锁 `self.db` 的服务方法。

## 验收标准

- [x] `cargo check --lib` 通过
- [x] `cargo clippy --lib` 改动文件零告警
- [x] node 模块测试 46 过（2 个失败为预存 workspace `\\?\` 路径问题，HEAD 同失败）
- [x] `pnpm build` 通过

## 进度

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ✅ | 根因：`runtime_list_unified_projects` 在持有 `state.db`（= RuntimeService 同一 Arc 的 Mutex）时调用 `state.runtime.list_projects()`，后者内部 `self.db.lock()` 二次加锁非重入 Mutex → 自死锁，死锁线程永占 DB 锁 → 全应用 IPC 排队无响应。修法：锁内直接 `query_dependency_graph(&conn)`（复用 `inspect_project_with_connection` 同款「避免递归锁」模式），扫描保持锁外。验证：cargo check/clippy 干净、node 46 过、pnpm build 过、brace-aware 全仓扫描无残留嵌套锁 |
