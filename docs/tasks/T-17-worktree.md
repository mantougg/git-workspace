# T-17 Worktree

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09 |
| 对应 Roadmap | §14 Worktree、§41 schema（worktrees） |

## 目标

实现 Git Worktree 管理：多工作目录并行开发，含创建/移除/切换/打开/建分支。

## 需求范围

- [ ] Worktree 列表：主仓库 + 各 worktree（main / feature/* / hotfix/*）
- [ ] 操作：Create Worktree / Remove Worktree / Checkout Worktree / Open Folder / Create Branch
- [ ] Worktree 落库 `worktrees`，与仓库关联展示
- [ ] Remove Worktree（含未合并变更时）走 §46 Warning 确认
- [ ] 每个 worktree 作为独立仓库入口参与状态/批量操作（复用 T-02）

## 架构 / 性能注意点

- worktree 本质是共享同一 `.git` 的多个工作目录，扫描/状态需识别 `.git` 为文件的 worktree 形态（当前 scanner 只识别 `.git` 目录，需扩展）。
- 多个 worktree 共享对象库，状态计算注意不要在 worktree 间重复扫描共享数据。

## 验收标准

- [ ] Worktree 正确创建/移除，主仓库状态一致
- [ ] `.git` 文件形态的 worktree 能被 scanner 正确识别（联动 T-01）
- [ ] Open Folder 打开正确目录
- [ ] 移除含未合并变更的 worktree 有确认提示

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] worktree 数据模型与落库
- [ ] 列表 UI 与操作命令
- [ ] scanner 识别 worktree 形态（联动 T-01）
- [ ] Open Folder / Checkout 集成
