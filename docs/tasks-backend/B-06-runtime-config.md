# B-06 拆 Runtime Config（config.rs → config/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.8、§6 Phase 4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §4.8 目标目录、§6 Phase 4、§2.2（config.rs 列为第二批） |

## 目标

把约 849 行生产代码的 `runtime/config.rs` 按「模型 / 元数据 / 环境合并 / 文件存储 / 校验」拆成 `runtime/config/` 子模块，保持现有持久化边界：SQLite 存元数据索引，`.gitworkspace/runtimes/*.json` 存用户配置。

## 需求范围

- [x] 目标结构（§4.8）：`config/{mod.rs, model.rs, repository.rs, environment.rs, storage.rs, validation.rs, tests.rs}`
- [x] `model.rs`：RuntimeApplicationConfig、请求和摘要 DTO
- [x] `repository.rs`：SQLite 元数据和配置生命周期
- [x] `environment.rs`：Global / Workspace / Runtime / Service 四层合并
- [x] `storage.rs`：JSON 读写、原子写、schema 默认值
- [x] `validation.rs`：名称、路径、符号链接和敏感字段校验
- [x] `mod.rs`：公共配置 API 与 re-export，调用方零修改

## 架构 / 性能注意点

- 持久化边界不变（§4.8）：SQLite 元数据 + JSON 文件双写语义、原子写（临时文件 + rename）行为保持。
- **拆分不能把完整秘密值重新暴露到 IPC**（§4.8）：摘要 DTO 的脱敏字段保持脱敏。
- 环境合并顺序（Global → Workspace → Runtime → Service）是纯逻辑，进 `environment.rs` 后应具备独立单测。
- JSON schema 默认值兼容旧配置文件（不破坏既有 `runtimes/*.json`）。

## 验收标准

- [x] 配置读写、合并、校验行为不变（既有测试全绿）
- [x] IPC 返回的敏感字段仍脱敏（测试断言）
- [x] 原子写行为不变（无半写文件风险）
- [x] 旧格式配置文件可直接读取（兼容性测试）
- [x] 四件套全绿；公共 API 路径不变

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：确认 `runtime/config.rs` 实际 999 行（spec 估算 849），测试仍内联于文件尾（本任务外移至 `config/tests.rs`）；外部经 `config::` 路径使用 `write_json_atomic` / `MASKED_VALUE` / `load_config_unredacted` / `workspace_root`，需按原可见性 re-export |
| 2026-08-29 | ✅ | 完成：`git mv` 保留历史后按 `model+validation → storage → repository(+environment) → mod 门面+测试外移` 迁移，每组跑四件套。999 行 → `mod.rs` 38 行（门面）+ 5 个子模块（model 185 / validation 103 / storage 189 / repository 353 / environment 96）+ tests.rs 156。GitNexus `impact`：`create_config` CRITICAL（20 直接调用方、11 执行流），经 mod.rs 按原可见性 re-export（`pub` / `pub(crate)`）实现调用方零修改，`detect_changes()` 风险 LOW、受影响执行流 0。验证：全量 `cargo test` 490 通过 / 3 ignored 与基线一致（仅 2 个既有 `maven::settings` 环境失败）；clippy 在 `runtime/config/` 零告警；5 个既有测试（含合并顺序、脱敏占位符保留、旧 JSON 兼容）全数保留并通过。注：`repository.rs` 与 `environment.rs` 同步迁移（`resolve_environment` 依赖 repository 的 `get_summary`，拆开会产生中间态悬空引用）。 |

### 子任务清单

- [x] `model.rs` + `validation.rs`（纯类型与校验）
- [x] `storage.rs`（JSON 原子写）
- [x] `repository.rs`（SQLite 元数据）
- [x] `environment.rs`（四层合并）
- [x] `mod.rs` 门面与 re-export
- [x] 测试归位与四件套验证
