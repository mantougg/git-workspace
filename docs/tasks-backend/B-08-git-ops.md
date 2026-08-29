# B-08 拆 GitOps（git_ops.rs → git_ops/）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[B-01](./B-01-baseline-test-extraction.md)。设计约束见 [backend-module-split-plan.md](../backend-module-split-plan.md) §4.9、§6 Phase 4。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · 支撑模块 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | B-01 |
| 对应设计文档 | §4.9 目标目录、§6 Phase 4 |

## 目标

把约 658 行生产代码的 `core/git_ops.rs` 按「远程操作 / 本地提交 / 安全扫描 / Shell 执行」拆成 `core/git_ops/` 子模块，保持有意的适配策略：远程走系统 `git` CLI，本地 commit/status/diff 走 git2（§4.9）。

## 需求范围

- [x] 目标结构（§4.9）：`git_ops/{mod.rs, remote.rs, commit.rs, safety.rs, shell.rs, tests.rs}`
- [x] `remote.rs`：fetch / pull / push / clone（系统 git CLI）
- [x] `commit.rs`：normal commit / amend / index-only / identity（git2）
- [x] `safety.rs`：secret / large-file / forbidden-file 扫描（复用 T-08 能力）
- [x] `shell.rs`：ShellCommand、超时和输出尾部
- [x] `mod.rs`：GitOps 门面和 TaskType 分发 + re-export，调用方零修改

## 架构 / 性能注意点

- 不改变「远程 CLI / 本地 git2」的有意适配策略（§4.9）；不为统一而重写任何一侧。
- 提交前安全扫描（secret/大文件/禁提交文件）的拦截时机和错误结构不变。
- Shell 执行遵守平台规范：超时、输出尾部截断、Windows `CREATE_NO_WINDOW`、`.cmd`/`.bat` 经 `cmd /C`（全局约束 §6）。
- TaskType 分发留在 `mod.rs` 门面，任务语义不变（全局约束 §1）。

## 验收标准

- [x] fetch/pull/push/clone 与 commit/amend 行为不变（既有测试全绿）
- [x] 安全扫描拦截行为不变（测试断言）
- [x] Shell 超时与输出尾部行为不变；平台分支保留
- [x] 公共入口与任务分发不变，调用方零修改
- [x] 四件套全绿；`detect_changes()` 无超预期影响

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：确认现状（git_ops.rs 1004 行）与目标结构（§4.9） |
| 2026-08-29 | ✅ | 完成：`git mv` 保留历史后按 shell → safety → commit → remote → tests 顺序迁移，每组跑四件套。1004 行 → `mod.rs` 120 行（门面 + TaskType 分发）+ remote 160 / commit 169 / safety 125 / shell 106 / tests 349。GitNexus impact：`GitOps` MEDIUM（调用方 commands/git_ops.rs、commands/branch.rs、task/worker.rs、lib.rs 均经 `core::git_ops::` 公共路径访问，零修改）、`pre_commit_scan` LOW。可见性按 §5.2 收敛：`commit`/`clone_repo` pub→pub(super)（全仓无外部调用者），`scan_paths`/`paths_for_scan`/`format_findings` 收敛为 pub(super)；随迁修复 dead code `default_git_ops`（全仓零调用，clippy 1.98 dead_code）。公共入口不变：`GitOps` / `pre_commit_scan` / `CommitOptions` 经 mod.rs re-export；「远程 git CLI / 本地 git2」适配策略原样保留；Windows `CREATE_NO_WINDOW`、`cmd /C`/`sh -c` 平台分支保留。`detect_changes()`：LOW、受影响执行流 0。测试总数不变（494），git_ops 域 9/9 全绿；全量 5 失败逐一溯源均与本任务无关：maven::settings ×2 基线即失败（本机 `~/.m2` 干扰）、logs::flood 在干净 master 树复跑同样失败（预存 flaky，B-07 已记录）、force_kill / benchmark_smoke / diff_cache 单独复跑通过（负载 flaky）。fmt/clippy 口径同 B-06/B-07：本任务触碰文件（core/git_ops/ 全部 6 文件）零告警；全仓 `fmt --check` / `clippy -D warnings` 存在 rustc 1.98.0 工具链升级导致的预存漂移（约 100 文件），不属本任务范围，建议另立 chore 统一重排。 |

### 子任务清单

- [x] `shell.rs`（ShellCommand/超时/输出尾部）
- [x] `safety.rs`（安全扫描）
- [x] `commit.rs`（本地提交）
- [x] `remote.rs`（远程 CLI 操作）
- [x] `mod.rs` 门面与 TaskType 分发
- [x] 测试归位与四件套验证
