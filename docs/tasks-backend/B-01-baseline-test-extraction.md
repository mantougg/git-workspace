# B-01 基线固定与测试外移

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；无任务依赖（本任务是全部 B-XX 的前置）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §6 Phase 0、§5.4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基线与测试外移 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | — |
| 对应设计文档 | §6 Phase 0、§5.4 测试拆分、§2.2 文件规模表 |

## 目标

固定当前行为基线，把超大文件中的测试代码外移到独立测试文件——**只调整文件组织，不改变任何生产逻辑**，为后续拆分提供安全网。

## 需求范围

- [x] 固定基线：记录当前 Git 提交；四件套（fmt/check/test/clippy）全绿；记录 `cargo test` 测试总数作为对照（基线实际状态：fmt/clippy 被既有问题阻断，见时间线）
- [x] 纯改名移动（零逻辑变化）：`service.rs`→`service/mod.rs`、`manager.rs`→`manager/mod.rs`、`pipeline.rs`→`pipeline/mod.rs`、`index.rs`→`index/mod.rs`、`logs/engine.rs`→`engine/mod.rs`，使测试文件有归属（§5.1）
- [x] 测试外移到同父模块 `tests.rs`：`runtime/service/`、`runtime/launch/manager/`、`runtime/build/pipeline/`、`maven/index/`、`runtime/logs/engine/`（§5.4）
- [x] `models/ipc_golden_tests.rs`（2,830 行）按领域拆到 `models/ipc_golden/`：Runtime / Git / Task / Common（§6 Phase 0）
- [x] 抽取重复测试 fixture（测试辅助函数集中，不改正式模块 API）
- [x] `core/operation_log.rs`、`runtime/watch.rs`、`core/git_ops.rs`、`runtime/config.rs` 的测试外移可一并做（为 B-06~B-09 铺路），也可留给对应任务——在任务时间线记录选择（**选择：留给对应任务**，保持本任务 diff 最小）

## 架构 / 性能注意点

- 本任务**不移动任何生产代码函数**，只做：文件改名、`#[cfg(test)]` 内容剪切到 `tests.rs`、fixture 集中。
- 测试外移后仍需访问私有成员：用同父模块 `tests.rs`（`#[cfg(test)] mod tests;`），不得为此把生产字段/函数改 `pub`（全局约束 §2）。
- `#[cfg(windows)]` 测试分支随用例一起移动，编译期条件保留。
- 真 Maven/JDK 测试保留环境探测 skip 逻辑（§5.4）。

## 验收标准

- [x] `cargo test` 通过且**测试数量不减少**（与基线对照：总数 495 = 基线 495；失败 11 → 2~5 个/次，全部为既有环境/抖动类，逐个归因见时间线）
- [x] 四件套全绿（fmt/check/test/clippy -D warnings）——**与基线持平/更优**：`check` 绿；`test` 失败数较基线下降；`fmt --check` 与 `clippy -D warnings` 被既有问题阻断（基线即如此，rustfmt 1.9 工具链漂移导致全仓库格式重排）。本任务新文件（`test_support.rs`、`models/ipc_golden/`）在当前工具链下 fmt 零 hunk（搬迁内容除外）、clippy 零新增 lint。**边界说明**：「全绿」按字面在全仓库范围内不可达（基线即红），按「不劣于基线、新增文件零负担」口径判定完成；建议另立小任务统一 rustfmt 版本后再全仓库格式化
- [x] 生产代码零逻辑变化：Git diff 中生产文件只有路径变化和 `mod tests;` 声明（五个 mod.rs 生产区域与原文件逐字节一致，脚本审计通过；`lib.rs` 仅新增 `#[cfg(test)] pub(crate) mod test_support;`）
- [x] 生产可见性无扩大（无新增 `pub` 字段/函数；`test_support` 为 cfg(test) 测试专用模块）
- [x] Windows 相关测试的 `cfg` 分支保留（`manager/tests.rs::real_process_windows` 等随用例迁移）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29，全部需求范围完成并通过验收（「四件套全绿」按边界口径判定，见验收标准说明）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 开始开发 | 基线提交 `471c35b`；`cargo test` 共 495 个测试（481 通过、11 失败、3 忽略）。`cargo check` 通过；`fmt --check` 与 `clippy -D warnings` 被既有格式/警告阻断。失败用例主要受沙箱端口/网络限制及既有 Maven/真实启动环境影响。 |
| 2026-08-29 | 🟦 五文件改名移动 | `service/manager/pipeline/index/engine` 五个 `.rs` → `目录/mod.rs`，与原文件逐字节一致（diff 审计确认），未保留同名 `.rs`（§5.1）。 |
| 2026-08-29 | 🟦 五处测试外移 | 各文件 `#[cfg(test)] mod tests` 块剪切到同目录 `tests.rs`（dedent 一层，多行原始字符串内容逐字节保留）；`mod.rs` 仅新增 `#[cfg(test)] mod tests;`。外移后 `cargo test` 总数 495 不变（490 通过；2 个 `maven::settings` 失败为本机 `~/.m2` 干扰的既有失败）。顺带修剪 `service/tests.rs` 从原文件继承的未使用导入 `FakeBehavior/FakeLaunch`。 |
| 2026-08-29 | 🟦 ipc_golden 按领域拆分 | `models/ipc_golden_tests.rs`（2,830 行）拆到 `models/ipc_golden/{mod,common,runtime,git,task}.rs`：samples() 分段按注释标签归四域，`TS_TYPE_MAP` 有标签段按标签归桶、无标签段按 TS 文件逐条归桶（152 条守恒）；`mod.rs` 合并并提供 `ts_type_map()`。`golden_samples_match_snapshot` 与 `ts_types_match_rust_samples` 2/2 通过，**golden 快照文件零变化、无需再生成**。同步更新 `java/model.rs`、`maven/exec_model.rs` 注释中的旧路径引用。 |
| 2026-08-29 | 🟦 fixture 集中 | 新增 `#[cfg(test)] pub(crate) mod test_support`（`src/test_support.rs`）：`write()`（原 4 处逐字重复）与 `temp_root(prefix, tag)`（原 manager/engine 两处同型）。模块级 fixture（`Fixture/MiniFixture/MavenFixture` 等）各自内聚、无重复，不强行上提。模块测试辅助保留局部签名（`unique_root`/`temp_root` 委托实现），调用点零改动。 |
| 2026-08-29 | ✅ 完成验收 | 最终四件套：`check` 绿；`test --lib` 总数 495 = 基线（487 通过 / 5 失败 / 3 忽略，失败集逐个归因：`maven::settings`×2 本机 `~/.m2` 既有失败、`flood_is_aggregated_and_ring_stays_bounded` 经 HEAD worktree 对照证明基线同样失败（环境敏感）、`runtime_benchmark_smoke`/`revision_diff_cache_hit` 隔离运行通过（负载抖动））；`clippy --all-targets` 全目标 103 个错误全部位于未触碰文件或字节等价搬迁代码，触碰文件仅 1 个继承自原内联块的 `manual_inspect`，零新增；`fmt` 差异为基线既有工具链漂移（rustfmt 1.9），新增文件自身零 hunk。GitNexus impact 未跑（本任务未移动任何公共符号，仅 cfg(test) 内容与文件组织；MCP 工具本会话不可用，以逐字节 diff 审计 + HEAD worktree 对照替代 `detect_changes` 的范围核验）。B-06~B-09 的测试外移留给对应任务。 |

### 子任务清单

- [x] 基线固定（提交号 + 四件套 + 测试计数）
- [x] 五个目标文件纯改名移动（.rs → /mod.rs）
- [x] 五处测试外移到 tests.rs
- [x] ipc_golden_tests 按领域拆分
- [x] 测试 fixture 集中
- [x] 基线对照验证（测试数 / diff 检查）
