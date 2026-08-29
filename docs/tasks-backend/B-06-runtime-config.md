# B-06 拆 Runtime Config（config.rs → config/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.8、§6 Phase 4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §4.8 目标目录、§6 Phase 4、§2.2（config.rs 列为第二批） |

## 目标

把约 849 行生产代码的 `runtime/config.rs` 按「模型 / 元数据 / 环境合并 / 文件存储 / 校验」拆成 `runtime/config/` 子模块，保持现有持久化边界：SQLite 存元数据索引，`.gitworkspace/runtimes/*.json` 存用户配置。

## 需求范围

- [ ] 目标结构（§4.8）：`config/{mod.rs, model.rs, repository.rs, environment.rs, storage.rs, validation.rs, tests.rs}`
- [ ] `model.rs`：RuntimeApplicationConfig、请求和摘要 DTO
- [ ] `repository.rs`：SQLite 元数据和配置生命周期
- [ ] `environment.rs`：Global / Workspace / Runtime / Service 四层合并
- [ ] `storage.rs`：JSON 读写、原子写、schema 默认值
- [ ] `validation.rs`：名称、路径、符号链接和敏感字段校验
- [ ] `mod.rs`：公共配置 API 与 re-export，调用方零修改

## 架构 / 性能注意点

- 持久化边界不变（§4.8）：SQLite 元数据 + JSON 文件双写语义、原子写（临时文件 + rename）行为保持。
- **拆分不能把完整秘密值重新暴露到 IPC**（§4.8）：摘要 DTO 的脱敏字段保持脱敏。
- 环境合并顺序（Global → Workspace → Runtime → Service）是纯逻辑，进 `environment.rs` 后应具备独立单测。
- JSON schema 默认值兼容旧配置文件（不破坏既有 `runtimes/*.json`）。

## 验收标准

- [ ] 配置读写、合并、校验行为不变（既有测试全绿）
- [ ] IPC 返回的敏感字段仍脱敏（测试断言）
- [ ] 原子写行为不变（无半写文件风险）
- [ ] 旧格式配置文件可直接读取（兼容性测试）
- [ ] 四件套全绿；公共 API 路径不变

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `model.rs` + `validation.rs`（纯类型与校验）
- [ ] `storage.rs`（JSON 原子写）
- [ ] `repository.rs`（SQLite 元数据）
- [ ] `environment.rs`（四层合并）
- [ ] `mod.rs` 门面与 re-export
- [ ] 测试归位与四件套验证
