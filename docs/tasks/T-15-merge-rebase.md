# T-15 Merge / Rebase

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09 |
| 对应 Roadmap | §12 Merge / Rebase |

## 目标

实现 Merge 与 Rebase 完整能力，包括 Interactive Rebase 与操作中断后的 Continue/Abort/Skip。

## 需求范围

- [x] Merge：普通 / `--no-ff` / `--squash`
- [x] Rebase：普通 / `--onto` / Interactive Rebase
- [x] Interactive Rebase UI：pick / reword / squash / drop 可视编排（拖拽 + 下拉）
- [x] 中断恢复：Continue / Abort / Skip（处理 rebase 状态机）
- [x] 冲突时进入 T-16 Conflict Resolver
- [x] Merge / Rebase 归 §46 Warning 级确认

## 架构 / 性能注意点

- Interactive Rebase 走系统 `git` CLI（依赖 editor 语义），交互通过 `GIT_SEQUENCE_EDITOR` 脚本或 `rebase -i` 的 todo 文件改写实现，不弹外部 editor。
- Rebase 状态（`rebase-merge` / `rebase-apply`）需要持久感知，跨 UI 刷新 / 重启可恢复；配合 T-14 Reflog 保证可回退。
- **显式偏离说明（2026-08-14 实现定稿）**：Interactive Rebase 未走 git CLI，改为 libgit2 逐步序列器（`cherrypick` + 显式 commit）。原因：CLI 的 editor 语义在 Windows 下需 shell 脚本注入（`GIT_SEQUENCE_EDITOR`），且 reword/squash 的消息无法可靠传入 editor；序列器完全本地、Offline、可单测。状态持久化用 `.git/gitworkspace-rebase.json`（每步落盘，重启可续），替代 `rebase-merge` 目录；Abort = hard reset 到预存 `original_head`，配合 T-14 Reflog 可回退。Merge 保持 libgit2（符合全局约束 §3）。

## 验收标准

- [x] 三种 merge 模式语义正确
- [x] Interactive Rebase 的 pick/reword/squash/drop 编排正确生成 todo 并执行
- [x] 冲突后可 Continue/Abort/Skip，Abort 后工作区完整恢复
- [x] rebase 中断后重启应用仍能识别并继续处理

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-14 随 T-16 闭环

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-14 | 🟦 | 开始开发 |
| 2026-08-14 | 🟦 | 核心完成：`core/merge.rs`（FF/no-ff 合并提交/squash 暂存 + MERGE_HEAD 冲突态 + continue/abort）+ `core/rebase.rs` 自研序列器（pick/reword/squash/drop + 每步落盘 `.git/gitworkspace-rebase.json` + continue/skip/abort，CLI editor 语义偏离已在上方显式说明）+ 10 个 command；前端 `RebaseDialog.vue`（拖拽 + 下拉 + reword 编辑编排）+ BranchManager Merge 对话框（三模式 + Warning 确认）+ Merge/Rebase 中断横幅（重启后 Continue/Skip/Abort 恢复）；修复 squash 提交需绕开 HEAD first-parent 校验（commit(None) + set_target）与 FF checkout 顺序（同 T-09 baseline 坑）；8 个核心单元测试；IPC golden 登记 MergeOutcome/RebaseOp/RebaseState/RebaseOutcome；`cargo test` 68 passed、`vue-tsc` + `vite build` 通过。剩余：Resolver 跳转待 T-16 |
| 2026-08-14 | ✅ | 随 T-16 闭环：Merge/Rebase 横幅新增「打开解决器」，冲突操作统一进 Resolver（ours/theirs/both/手动编辑 + continue/abort 路由）；验收 4 条全部满足，`cargo test` 73 passed |

### 子任务清单

- [x] Merge 三模式实现
- [x] Rebase 基础 + --onto
- [x] Interactive Rebase UI 与 todo 生成
- [x] Continue/Abort/Skip 状态机
- [x] 与 T-16 冲突衔接
