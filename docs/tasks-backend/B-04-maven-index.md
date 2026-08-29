# B-04 拆 Maven Index（index.rs → index/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.4、§6 Phase 3。GitNexus：移动公共符号前必须跑 `impact`。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 数据索引与日志引擎 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 Maven index.rs 问题、§4.4 目标目录、§6 Phase 3、§7 数据库边界 |

## 目标

把约 1,000 行生产代码的 `maven/index.rs` 按「类型 / 缓存 / 事务同步 / 查询 / Source Mapping / 路径」拆成 `maven/index/` 子模块，隔离 SQLite 事务写入与查询边界。`maven::sync_workspace_index` 等公共路径不变。

## 需求范围

- [ ] 目标结构（§4.4）：`index/{mod.rs, types.rs, cache.rs, sync.rs, query.rs, mapping.rs, path.rs, tests.rs}`
- [ ] 迁移顺序（§6 Phase 3）：先拆 `types.rs` / `path.rs` / `query.rs`（低耦合），再拆 `sync.rs` 事务同步（最高风险），`mapping.rs` / `cache.rs` 居中
- [ ] `types.rs`：MavenProjectNode、DependencyGraph、SourceMapping
- [ ] `cache.rs`：DependencyGraphCache 与 fingerprint
- [ ] `sync.rs`：`sync_workspace_index` 及事务写入（项目、父子模块、依赖、artifact、source mapping）
- [ ] `query.rs`：graph / project / dependency / module 查询
- [ ] `mapping.rs`：Source Mapping、Artifact 刷新和清理
- [ ] `path.rs`：`path_key`、Windows verbatim path 清理、lexical normalize
- [ ] re-export 兼容（§5.3）：`maven::sync_workspace_index`、`maven::query_dependency_graph`、`maven::DependencyGraphCache`

## 架构 / 性能注意点

- `sync.rs` 是风险最高的部分（§4.4），必须继续保持：同一事务语义更新、失败不留半套索引、索引变化后 `graph_cache` 与 `closure_cache` 失效。
- 缓存失效由同步用例统一负责，`query.rs` 不得直接修改缓存失效状态（§7）。
- 路径比较统一经 `path.rs` 归一化：正斜杠 + `\\?\` verbatim 前缀清理；禁止裸 `==`（全局约束 §6）。
- DB 写操作保持短事务，不在同步期间持有锁执行外部命令（§7）。

## 验收标准

- [ ] Maven 索引同步原子性测试继续通过（失败回滚不留半套索引）
- [ ] Cache fingerprint 与失效时机不变（既有缓存测试全绿）
- [ ] Windows 混合分隔符与 verbatim 前缀路径比较测试保留并通过
- [ ] 公共 re-export 路径不变，调用方零修改
- [ ] 四件套全绿；`detect_changes()` 无超预期影响；AGENTS.md 中 `index.rs::path_key` 等引用已同步

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `types.rs` + `path.rs`（纯类型与路径工具）
- [ ] `cache.rs`
- [ ] `query.rs`
- [ ] `mapping.rs`
- [ ] `sync.rs`（事务同步，最后移）
- [ ] 测试归位与四件套验证 + 文档同步
