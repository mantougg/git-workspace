# T-12 Diff 增强（Hunk / Line Stage + 多对象 Diff）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-04 |
| 对应 Roadmap | §9 Diff 增强 |

## 目标

将 Diff 从「只读查看」升级为「可交互暂存」：Hunk / Line 级 Stage/Unstage，并支持多种 Diff 对象（File/Hunk/Line/Commit/Branch/Tag/双点）。

## 需求范围

- [ ] Diff 层级：File Diff / Hunk Diff / Line Diff
- [ ] 对象间 Diff：Commit Diff / Branch Diff / Tag Diff / Commit A↔B / Branch A↔B
- [ ] Stage Hunk / Unstage Hunk / Stage Line / Unstage Line（libgit2 patch/stage）
- [ ] 与 T-11 联动：暂存的 hunk/line 参与 Commit
- [ ] 保持 Unified 与 Side-by-Side 两种视图（已有）

## 架构 / 性能注意点

- line 级暂存依赖 libgit2 的 `Patch` 与 index stage 操作；line 数量大时按 hunk 分块处理。
- 双点 Diff（A↔B）复用 diff 缓存，key 用 `(old_oid, new_oid, path)`。
- 暂存状态变化要即时反映到状态缓存（T-02 失效对应仓库 status）。

## 验收标准

- [ ] 单行 stage/unstage 后，工作区/暂存区状态与 `git diff --cached` 一致
- [ ] Commit/Branch/Tag/双点 Diff 四类对象均可用
- [ ] 大文件 line 级操作不卡死、内存可控

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Hunk / Line 级 Stage/Unstage 实现
- [ ] 多对象 Diff 入口与查询
- [ ] 暂存操作与状态缓存联动
- [ ] 与 T-11 Commit 集成
