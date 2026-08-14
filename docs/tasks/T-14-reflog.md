# T-14 Reflog

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09 |
| 对应 Roadmap | §11 Reflog |

## 目标

实现 Reflog 查看与恢复操作，降低 Reset / Rebase / 误删除等高风险操作的恢复成本。

## 需求范围

- [x] 三种 reflog：HEAD / Branch / Remote
- [x] 列表展示 `HEAD@{0} / HEAD@{1} / ...` 及对应 commit 摘要
- [x] 操作：Create Branch Here / Reset Here / Restore State / View Commit
- [x] Reset Here / Restore State 走 §46 Warning/Dangerous 确认
- [x] 与 T-13 Reset 联动：reset 前提示可记录 reflog 位置

## 架构 / 性能注意点

- reflog 读取是本地轻量操作，按需分页加载即可，无需缓存。
- 恢复操作本质是 reset，复用 T-13 的危险确认与影响范围展示。

## 验收标准

- [x] HEAD/Branch/Remote 三类 reflog 正确展示
- [x] Create Branch Here / Reset Here 可用且可回退
- [x] 误 reset 后可通过 reflog 恢复原状态

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发 |
| 2026-08-14 | ✅ | 完成：新增 `core/reflog.rs`（HEAD/refs/heads/refs/remotes 读取 + selector/摘要/时间，未知 ref 结构化 NotFound）+ `get_reflog` command；恢复操作直接复用 T-09 `create_branch` 与 T-13 `reset_to`；前端 `Reflog.vue`（HEAD/本地/远程选择器 + selector 列表 + View Commit / Create Branch Here / Reset Here 三档 / Restore State 危险确认）+ GitGraph 头部 Reflog 入口 + T-13 reset 成功提示挂 Reflog 指引；3 个单元测试（含「误 reset 经 reflog 恢复」验收场景）；IPC golden 登记 ReflogEntry；`cargo test` 56 passed、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] reflog 读取与列表 UI
- [x] 恢复类操作与确认
- [x] 与 T-13 联动
