# T-13 Cherry-pick / Revert / Reset

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09 |
| 对应 Roadmap | §10 History 操作、§46 Git Operation Safety |

## 目标

补齐高风险历史操作：Cherry-pick、Revert、Reset，均纳入 §46 分级确认，降低误操作代价。

## 需求范围

- [ ] Cherry-pick：单 commit / 多 commit，冲突时进入 T-16 Conflict Resolver
- [ ] Revert：单个 commit revert，生成 revert 提交
- [ ] Reset：soft / mixed / hard 三档，hard 归为 Dangerous 级二次确认，明确显示影响范围（仓库/分支/文件/潜在数据丢失）
- [ ] 入口：History 的 commit 上下文菜单 + Command Palette
- [ ] 操作结果反馈：成功 / 冲突（跳转 T-16）/ 失败（结构化错误）

## 架构 / 性能注意点

- Reset --hard / Clean / Force Push 归 Dangerous（§46），确认弹窗必须列出影响范围与潜在数据丢失；`reset --hard` 前建议提示可先 stash 或记下 reflog 位置。
- Cherry-pick / Revert 冲突后要保持仓库处于可恢复状态，配合 T-14 Reflog 与 T-16 冲突解决。

## 验收标准

- [ ] 三档 reset 语义正确，hard 有明确危险确认
- [ ] Cherry-pick 冲突进入 Conflict Resolver 且可 abort 恢复
- [ ] Revert 生成正确 revert 提交
- [ ] 所有危险操作确认弹窗包含仓库/分支/影响文件/数据丢失提示

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Cherry-pick 实现与冲突衔接
- [ ] Revert 实现
- [ ] Reset 三档 + 危险确认
- [ ] History 上下文菜单接入
