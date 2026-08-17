# T-17 Worktree

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 完整 Git Client（P0） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09 |
| 对应 Roadmap | §14 Worktree、§41 schema（worktrees） |

## 目标

实现 Git Worktree 管理：多工作目录并行开发，含创建/移除/切换/打开/建分支。

## 需求范围

- [x] Worktree 列表：主仓库 + 各 worktree（`list_worktrees`：main + `Repository::worktrees()`，含分支/锁定/脏状态）
- [x] 操作：Create Worktree / Remove Worktree / Checkout Worktree（进入该 worktree 的 Graph/Diff 上下文）/ Open Folder（plugin-shell）/ Create Branch（创建对话框内 new_branch）
- [x] Worktree 落库 `worktrees`，与仓库关联展示（`list_worktrees` 时 `replace_worktrees` 快照）
- [x] Remove Worktree（含未合并变更时）走 §46 Warning 确认（脏检查 → 二次确认 → force）
- [x] 每个 worktree 作为独立仓库入口参与状态/批量操作（scanner 识别 `.git` 文件形态后出现在仓库列表，复用 T-02）

## 架构 / 性能注意点

- worktree 本质是共享同一 `.git` 的多个工作目录，扫描/状态需识别 `.git` 为文件的 worktree 形态（当前 scanner 只识别 `.git` 目录，需扩展）。
- 多个 worktree 共享对象库，状态计算注意不要在 worktree 间重复扫描共享数据。

## 验收标准

- [x] Worktree 正确创建/移除，主仓库状态一致（`add_and_list_worktrees` / `remove_dirty_worktree_requires_force` / `remove_prunes_externally_deleted_worktree`）
- [x] `.git` 文件形态的 worktree 能被 scanner 正确识别（`scan_discovers_worktree_gitfile_form`，联动 T-01）
- [x] Open Folder 打开正确目录（`@tauri-apps/plugin-shell` 的 `open(path)`，`shell:allow-open` 已在 capabilities）
- [x] 移除含未合并变更的 worktree 有确认提示（脏检查拒绝 → §46 二次 Warning 确认 → force 移除）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 全部验收标准通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发 |
| 2026-08-17 | ✅ | 完成：`core/worktree.rs`（list/add/remove + 脏检查 + 外部删除后 prune 兜底）；scanner 支持 `.git` 文件形态（worktree/submodule 检出，T-01 联动）+ 测试；`replace_worktrees` 落库快照 + commands（list/create/remove）；前端 WorktreeManager 视图（列表/创建对话框含建分支/移除二次确认/打开目录/进入 Graph-Diff）+ 路由 + 仓库列表入口；golden/TS 同步；`cargo test` 100 passed、`pnpm build` 通过 |

### 子任务清单

- [x] worktree 数据模型与落库
- [x] 列表 UI 与操作命令
- [x] scanner 识别 worktree 形态（联动 T-01）
- [x] Open Folder / Checkout 集成
