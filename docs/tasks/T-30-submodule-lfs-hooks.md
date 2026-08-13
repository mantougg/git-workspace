# T-30 Submodule / LFS / Hooks

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 6（P2） |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-02 |
| 对应 Roadmap | §30 Submodule、§31 Git LFS、§29 Git Hooks |

## 目标

补齐 Submodule、Git LFS、Git Hooks 三类能力，覆盖大型工程常见场景。

## 需求范围

- [ ] Submodule：Init / Update / Sync / Status / Add / Remove，列表展示 synced/modified 状态
- [ ] Git LFS：LFS Status / Fetch / Pull / Push / Locks
- [ ] Git Hooks：pre-commit / prepare-commit-msg / commit-msg / post-commit / pre-push / post-checkout / post-merge 的 View / Edit / Run / Enable / Disable

## 架构 / 性能注意点

- Submodule 状态是递归/较重操作，按需计算并缓存，不进常驻 status 路径（健康检测 T-19 也如此）。
- LFS 走系统 `git lfs` CLI，与现有网络操作一致；Hook 编辑直接读写 `.git/hooks` 文件，注意权限与编码。

## 验收标准

- [ ] Submodule 六类操作可用，状态展示正确
- [ ] LFS status/fetch/pull/push/locks 可用
- [ ] Hooks 查看/编辑/启停/运行可用，编辑后立即生效

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Submodule 操作与状态
- [ ] LFS 操作
- [ ] Hooks 管理
