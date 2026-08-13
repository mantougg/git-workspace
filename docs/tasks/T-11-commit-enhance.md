# T-11 Commit 增强

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-04 |
| 对应 Roadmap | §8 Commit 增强 |

## 目标

在现有「按仓库批量 Commit」基础上增强为完整 Commit 能力：Amend、部分提交（Selected/Hunk/Line）、Commit & Push。

## 需求范围

- [ ] Commit / Amend / Commit --no-edit
- [ ] Commit Selected（勾选文件）
- [ ] Commit Hunk / Commit Line（依赖 T-12 的 hunk/line 暂存）
- [ ] Commit & Push（提交后走网络 Push，进 T-05 任务队列）
- [ ] Commit 前安全检查：Secret Scan + Large File Scan + Forbidden File Scan（T-08 提供）
- [ ] Commit UI：变更树勾选 → hunk/line 勾选 → message → [Commit] / [Commit & Push]
- [ ] Per-repo 提交身份：按仓库/分组配置 identity（name/email），Commit 自动选用（Roadmap 评审增量，§54）

## 架构 / 性能注意点

- hunk/line 级提交依赖 libgit2 的 patch/stage 能力；大文件 line 级操作要限制规模，避免内存爆炸。
- Commit 前安全检查是同步拦截，规则轻量、按文件 mtime 缓存（T-08）。
- 批量 Commit 走 T-05 任务队列并保持「按仓库分别提交」语义。

## 验收标准

- [ ] Amend / --no-edit 语义正确
- [ ] 只提交勾选的 hunk / line，未勾选部分保持未暂存
- [ ] Commit & Push 失败时正确区分「提交成功但推送失败」的中间态
- [ ] 含 `.env` / 大文件时被安全检查拦截并可放行
- [ ] 不同仓库/分组使用各自提交身份，切换无感

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Amend / --no-edit 实现
- [ ] 勾选文件提交（已有，补全）
- [ ] Hunk / Line 级提交（依赖 T-12）
- [ ] Commit & Push 与中间态处理
- [ ] 接入 Commit 前安全检查
- [ ] Per-repo 提交身份配置与选用
