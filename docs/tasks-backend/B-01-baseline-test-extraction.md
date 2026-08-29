# B-01 基线固定与测试外移

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；无任务依赖（本任务是全部 B-XX 的前置）。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §6 Phase 0、§5.4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基线与测试外移 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | — |
| 对应设计文档 | §6 Phase 0、§5.4 测试拆分、§2.2 文件规模表 |

## 目标

固定当前行为基线，把超大文件中的测试代码外移到独立测试文件——**只调整文件组织，不改变任何生产逻辑**，为后续拆分提供安全网。

## 需求范围

- [ ] 固定基线：记录当前 Git 提交；四件套（fmt/check/test/clippy）全绿；记录 `cargo test` 测试总数作为对照
- [ ] 纯改名移动（零逻辑变化）：`service.rs`→`service/mod.rs`、`manager.rs`→`manager/mod.rs`、`pipeline.rs`→`pipeline/mod.rs`、`index.rs`→`index/mod.rs`、`logs/engine.rs`→`engine/mod.rs`，使测试文件有归属（§5.1）
- [ ] 测试外移到同父模块 `tests.rs`：`runtime/service/`、`runtime/launch/manager/`、`runtime/build/pipeline/`、`maven/index/`、`runtime/logs/engine/`（§5.4）
- [ ] `models/ipc_golden_tests.rs`（2,830 行）按领域拆到 `models/ipc_golden/`：Runtime / Git / Task / Common（§6 Phase 0）
- [ ] 抽取重复测试 fixture（测试辅助函数集中，不改正式模块 API）
- [ ] `core/operation_log.rs`、`runtime/watch.rs`、`core/git_ops.rs`、`runtime/config.rs` 的测试外移可一并做（为 B-06~B-09 铺路），也可留给对应任务——在任务时间线记录选择

## 架构 / 性能注意点

- 本任务**不移动任何生产代码函数**，只做：文件改名、`#[cfg(test)]` 内容剪切到 `tests.rs`、fixture 集中。
- 测试外移后仍需访问私有成员：用同父模块 `tests.rs`（`#[cfg(test)] mod tests;`），不得为此把生产字段/函数改 `pub`（全局约束 §2）。
- `#[cfg(windows)]` 测试分支随用例一起移动，编译期条件保留。
- 真 Maven/JDK 测试保留环境探测 skip 逻辑（§5.4）。

## 验收标准

- [ ] `cargo test` 通过且**测试数量不减少**（与基线对照）
- [ ] 四件套全绿（fmt/check/test/clippy -D warnings）
- [ ] 生产代码零逻辑变化：Git diff 中生产文件只有路径变化和 `mod tests;` 声明（§9 diff 检查）
- [ ] 生产可见性无扩大（无新增 `pub` 字段/函数）
- [ ] Windows 相关测试的 `cfg` 分支保留

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 基线固定（提交号 + 四件套 + 测试计数）
- [ ] 五个目标文件纯改名移动（.rs → /mod.rs）
- [ ] 五处测试外移到 tests.rs
- [ ] ipc_golden_tests 按领域拆分
- [ ] 测试 fixture 集中
- [ ] 基线对照验证（测试数 / diff 检查）
