# T-15 Merge / Rebase

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09 |
| 对应 Roadmap | §12 Merge / Rebase |

## 目标

实现 Merge 与 Rebase 完整能力，包括 Interactive Rebase 与操作中断后的 Continue/Abort/Skip。

## 需求范围

- [ ] Merge：普通 / `--no-ff` / `--squash`
- [ ] Rebase：普通 / `--onto` / Interactive Rebase
- [ ] Interactive Rebase UI：pick / reword / squash / drop 可视编排（拖拽 + 下拉）
- [ ] 中断恢复：Continue / Abort / Skip（处理 rebase 状态机）
- [ ] 冲突时进入 T-16 Conflict Resolver
- [ ] Merge / Rebase 归 §46 Warning 级确认

## 架构 / 性能注意点

- Interactive Rebase 走系统 `git` CLI（依赖 editor 语义），交互通过 `GIT_SEQUENCE_EDITOR` 脚本或 `rebase -i` 的 todo 文件改写实现，不弹外部 editor。
- Rebase 状态（`rebase-merge` / `rebase-apply`）需要持久感知，跨 UI 刷新 / 重启可恢复；配合 T-14 Reflog 保证可回退。

## 验收标准

- [ ] 三种 merge 模式语义正确
- [ ] Interactive Rebase 的 pick/reword/squash/drop 编排正确生成 todo 并执行
- [ ] 冲突后可 Continue/Abort/Skip，Abort 后工作区完整恢复
- [ ] rebase 中断后重启应用仍能识别并继续处理

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Merge 三模式实现
- [ ] Rebase 基础 + --onto
- [ ] Interactive Rebase UI 与 todo 生成
- [ ] Continue/Abort/Skip 状态机
- [ ] 与 T-16 冲突衔接
