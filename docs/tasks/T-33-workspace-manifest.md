# T-33 Workspace Manifest + 批量 Clone

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-01 Scanner 硬化](./T-01-scanner.md)、[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-01, T-05 |
| 对应 Roadmap | §60 导入/导出 Workspace、§61 Workspace 模板、§62 Git Repository 模板 |

## 目标

把 §60 的 `gitworkspace.json` 升级为可重建环境的 **Workspace Manifest**（含 remote URL / 默认分支 / 分组 / 标签），Import 时可选批量 Clone，实现「新成员 10 分钟搭好环境」。

## 需求范围

- [ ] Manifest 导出：每个仓库的 remote URL + 默认分支 + 分组 + 标签（现 §60 的 `repositories` 为空，无法重建）
- [ ] Import 时批量 Clone（走 T-05 任务队列，逐仓库子结果 + Partial Success）
- [ ] 新成员入职引导：导入 Manifest → 批量 Clone → 加入 workspace
- [ ] 复用 T-01 Scanner 的扫描/验证逻辑与 T-05 的并发/进度能力

## 架构 / 性能注意点

- Clone 走系统 git CLI（网络操作边界），遵守 §45 并发限流（Fetch 8）。
- Manifest 只存纯数据（URL / 分支 / 分组），不存凭据；凭据走系统 credential 机制。
- 批量 Clone 的部分失败必须可定位、可重试，复用 T-05 Partial Success。

## 验收标准

- [ ] 导出的 Manifest 含 remote URL / 默认分支 / 分组，可据此重建环境
- [ ] 批量 Clone 100 仓库并发受限，部分失败可定位并重试
- [ ] 新成员用 Manifest 搭建环境的端到端流程可走通

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Manifest schema 扩展（remote URL / 分支 / 分组 / 标签）
- [ ] 导出端写入完整仓库信息
- [ ] Import 批量 Clone 接入任务队列
- [ ] 入职引导流程 UI
