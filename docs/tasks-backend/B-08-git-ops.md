# B-08 拆 GitOps（git_ops.rs → git_ops/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.9、§6 Phase 4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | B-01 |
| 对应设计文档 | §4.9 目标目录、§6 Phase 4 |

## 目标

把约 658 行生产代码的 `core/git_ops.rs` 按「远程操作 / 本地提交 / 安全扫描 / Shell 执行」拆成 `core/git_ops/` 子模块，保持有意的适配策略：远程走系统 `git` CLI，本地 commit/status/diff 走 git2（§4.9）。

## 需求范围

- [ ] 目标结构（§4.9）：`git_ops/{mod.rs, remote.rs, commit.rs, safety.rs, shell.rs, tests.rs}`
- [ ] `remote.rs`：fetch / pull / push / clone（系统 git CLI）
- [ ] `commit.rs`：normal commit / amend / index-only / identity（git2）
- [ ] `safety.rs`：secret / large-file / forbidden-file 扫描（复用 T-08 能力）
- [ ] `shell.rs`：ShellCommand、超时和输出尾部
- [ ] `mod.rs`：GitOps 门面和 TaskType 分发 + re-export，调用方零修改

## 架构 / 性能注意点

- 不改变「远程 CLI / 本地 git2」的有意适配策略（§4.9）；不为统一而重写任何一侧。
- 提交前安全扫描（secret/大文件/禁提交文件）的拦截时机和错误结构不变。
- Shell 执行遵守平台规范：超时、输出尾部截断、Windows `CREATE_NO_WINDOW`、`.cmd`/`.bat` 经 `cmd /C`（全局约束 §6）。
- TaskType 分发留在 `mod.rs` 门面，任务语义不变（全局约束 §1）。

## 验收标准

- [ ] fetch/pull/push/clone 与 commit/amend 行为不变（既有测试全绿）
- [ ] 安全扫描拦截行为不变（测试断言）
- [ ] Shell 超时与输出尾部行为不变；平台分支保留
- [ ] 公共入口与任务分发不变，调用方零修改
- [ ] 四件套全绿；`detect_changes()` 无超预期影响

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] `shell.rs`（ShellCommand/超时/输出尾部）
- [ ] `safety.rs`（安全扫描）
- [ ] `commit.rs`（本地提交）
- [ ] `remote.rs`（远程 CLI 操作）
- [ ] `mod.rs` 门面与 TaskType 分发
- [ ] 测试归位与四件套验证
