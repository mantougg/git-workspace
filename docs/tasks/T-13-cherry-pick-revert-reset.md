# T-13 Cherry-pick / Revert / Reset

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09 |
| 对应 Roadmap | §10 History 操作、§46 Git Operation Safety |

## 目标

补齐高风险历史操作：Cherry-pick、Revert、Reset，均纳入 §46 分级确认，降低误操作代价。

## 需求范围

- [x] Cherry-pick：单 commit / 多 commit（API 支持多 commit 顺序应用；UI 入口当前为单 commit），冲突时进入 T-16 Conflict Resolver
- [x] Revert：单个 commit revert，生成 revert 提交
- [x] Reset：soft / mixed / hard 三档，hard 归为 Dangerous 级二次确认，明确显示影响范围（仓库/分支/文件/潜在数据丢失）
- [x] 入口：History 的 commit 上下文菜单（CommitGraph 行内操作菜单）；Command Palette 入口待 T-31
- [x] 操作结果反馈：成功 / 冲突（进入 T-16 Resolver）/ 失败（结构化错误）

## 架构 / 性能注意点

- Reset --hard / Clean / Force Push 归 Dangerous（§46），确认弹窗必须列出影响范围与潜在数据丢失；`reset --hard` 前建议提示可先 stash 或记下 reflog 位置。
- Cherry-pick / Revert 冲突后要保持仓库处于可恢复状态，配合 T-14 Reflog 与 T-16 冲突解决。

## 验收标准

- [x] 三档 reset 语义正确，hard 有明确危险确认
- [x] Cherry-pick 冲突进入 Conflict Resolver 且可 abort 恢复（T-16 落地后闭环：横幅/对话框可进入解决器，pick_continue / abort_pick 衔接）
- [x] Revert 生成正确 revert 提交
- [x] 所有危险操作确认弹窗包含仓库/分支/影响文件/数据丢失提示

## 进度

### 状态

- 当前状态：已完成（Command Palette 入口待 T-31、UI 多 commit 选择器为后续增强，不影响验收）
- 最近更新：2026-08-14 随 T-16 闭环

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发 |
| 2026-08-14 | 🟦 | 核心完成：新增 `core/history.rs`（cherry_pick 多 commit 顺序应用/冲突保留现场、revert 生成回滚提交、reset soft/mixed/hard、abort_pick 恢复 + conflict_files 检测）+ `commands/history.rs` 5 个 command；前端 CommitGraph 行内操作菜单（Cherry-pick/Revert/Reset）+ GitGraph §46 确认流（hard reset Dangerous 列出仓库/分支/目标/数据丢失 + 原 HEAD 保底提示）+ 冲突对话框（文件列表 + Abort）+ 载入时冲突横幅；4 个核心单元测试；IPC golden 新增 PickOutcome/ResetResult（union 解析器支持 `status` 判别字段与多行 variant）；`cargo test` 53 passed、`vue-tsc` + `vite build` 通过。剩余：Resolver 跳转待 T-16、Palette 入口待 T-31、UI 多 commit 选择器 |
| 2026-08-14 | ✅ | 随 T-16 闭环：冲突横幅新增「进入解决器」，Resolver 内 cherry-pick/revert 经 `pick_continue` 继续、`abort_pick` 中止（有测试）；验收 4 条全部满足，`cargo test` 73 passed |

### 子任务清单

- [x] Cherry-pick 实现与冲突衔接（冲突检测 + Abort 恢复；Resolver 跳转待 T-16）
- [x] Revert 实现
- [x] Reset 三档 + 危险确认
- [x] History 上下文菜单接入（Command Palette 入口待 T-31）
