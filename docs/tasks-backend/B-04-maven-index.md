# B-04 拆 Maven Index（index.rs → index/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)（测试已外移）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.4、§6 Phase 3。GitNexus：移动公共符号前必须跑 `impact`。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 数据索引与日志引擎 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §2.2 Maven index.rs 问题、§4.4 目标目录、§6 Phase 3、§7 数据库边界 |

## 目标

把约 1,000 行生产代码的 `maven/index.rs` 按「类型 / 缓存 / 事务同步 / 查询 / Source Mapping / 路径」拆成 `maven/index/` 子模块，隔离 SQLite 事务写入与查询边界。`maven::sync_workspace_index` 等公共路径不变。

## 需求范围

- [x] 目标结构（§4.4）：`index/{mod.rs, types.rs, cache.rs, sync.rs, query.rs, mapping.rs, path.rs, tests.rs}`
- [x] 迁移顺序（§6 Phase 3）：先拆 `types.rs` / `path.rs` / `query.rs`（低耦合），再拆 `sync.rs` 事务同步（最高风险），`mapping.rs` / `cache.rs` 居中
- [x] `types.rs`：MavenProjectNode、DependencyGraph、SourceMapping
- [x] `cache.rs`：DependencyGraphCache 与 fingerprint
- [x] `sync.rs`：`sync_workspace_index` 及事务写入（项目、父子模块、依赖、artifact、source mapping）
- [x] `query.rs`：graph / project / dependency / module 查询
- [x] `mapping.rs`：Source Mapping、Artifact 刷新和清理
- [x] `path.rs`：`path_key`、Windows verbatim path 清理、lexical normalize
- [x] re-export 兼容（§5.3）：`maven::sync_workspace_index`、`maven::query_dependency_graph`、`maven::DependencyGraphCache`

## 架构 / 性能注意点

- `sync.rs` 是风险最高的部分（§4.4），必须继续保持：同一事务语义更新、失败不留半套索引、索引变化后 `graph_cache` 与 `closure_cache` 失效。
- 缓存失效由同步用例统一负责，`query.rs` 不得直接修改缓存失效状态（§7）。
- 路径比较统一经 `path.rs` 归一化：正斜杠 + `\\?\` verbatim 前缀清理；禁止裸 `==`（全局约束 §6）。
- DB 写操作保持短事务，不在同步期间持有锁执行外部命令（§7）。

## 验收标准

- [x] Maven 索引同步原子性测试继续通过（失败回滚不留半套索引）
- [x] Cache fingerprint 与失效时机不变（既有缓存测试全绿）
- [x] Windows 混合分隔符与 verbatim 前缀路径比较测试保留并通过
- [x] 公共 re-export 路径不变，调用方零修改
- [x] 四件套全绿；`detect_changes()` 无超预期影响；AGENTS.md 中 `index.rs::path_key` 等引用已同步

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：确认 B-01 已将测试外移至 `index/tests.rs`（生产代码 1013 行仍在 `index/mod.rs`）；基线 `cargo check` 通过 |
| 2026-08-29 | ✅ | 完成：按 `types+path → query → cache → mapping → sync` 顺序逐职责组迁移，每组跑四件套。生产代码 1013 行 → `mod.rs` 36 行（仅声明与 re-export）+ 6 个子模块（types 91 / path 42 / query 186 / cache 114 / mapping 207 / sync 449）。GitNexus `impact`：`sync_workspace_index` / `query_dependency_graph` / `refresh_dependency_sources` 均 CRITICAL（调用热点），经 mod.rs re-export 实现调用方零修改，`detect_changes()` 风险 LOW、受影响执行流 0。验证：全量 `cargo test` 490 通过 / 3 ignored，与基线一致（失败仅 2 个 `maven::settings` 环境泄漏用例，HEAD 上即失败，与本任务无关）；clippy 在 `maven/index/` 零告警。AGENTS.md 参照实现更新为 `maven/index/path.rs::path_key`。 |

**边界决策记录**（spec 口径澄清，遵循维护规范 6）：spec 中 `sync.rs`「事务写入含 source mapping」与 `mapping.rs`「Source Mapping、Artifact 刷新和清理」存在重叠。实际落点：`mapping.rs` 持有 Source Mapping 读写与比对（`mapping_fingerprint_rows` / `mapping_row_key` / `replace_source_mappings`）及 Artifact 刷新与清理（`refresh_dependency_sources` / `prune_artifacts`）；`sync.rs` 持有 `sync_workspace_index` 编排与项目/父子模块/依赖/artifact 事务写入。依赖方向：sync → mapping（函数）、mapping → sync（`ProjectInput`/`ProjectRecord`/`load_project_records`，均为 `pub(super)`）。

**环境备注**（非本任务引入）：当前工具链 rustfmt/clippy 1.98（rustfmt 1.9.0，2026-08-18 构建）对仓库约 100 个未改动文件产生格式漂移并新增既有 lint 告警，仓库级 `cargo fmt --check` / `clippy -D warnings` 在 HEAD 上即不通过；本任务按「diff 只允许预期文件变化」约束仅保证 `maven/index/` 范围内 fmt/clippy 全绿，仓库级 reformat 需单独决策。另有 `runtime::logs::engine::tests::flood_is_aggregated_and_ring_stays_bounded` 时序敏感测试偶发抖动（单独复跑 3 次 2 失败 1 通过），与 maven/index 无依赖。

### 子任务清单

- [x] `types.rs` + `path.rs`（纯类型与路径工具）
- [x] `cache.rs`
- [x] `query.rs`
- [x] `mapping.rs`
- [x] `sync.rs`（事务同步，最后移）
- [x] 测试归位与四件套验证 + 文档同步
