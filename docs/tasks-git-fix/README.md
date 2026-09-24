# GitWorkspace Git 功能体验修复任务总览（tasks-git-fix）

> 来源：2026-09-24 Git 使用体验全景盘点（前端 / 后端 / 文档三路探索 + 关键结论源码抽验，每条结论均有 file:line 或 文档:行 背书）。
> 编号规则：`GF-XX`（Git-Fix），一个独立问题一个文档（`GF-XX-<slug>.md`），可独立跟踪修复进度与验收。
> 本文件是唯一的进度索引；每个任务文档内另有自己的「进度」章节。
>
> 横切约束不重复记录：Git 功能相关遵守 [docs/tasks/00-全局开发约束.md](../tasks/00-全局开发约束.md)，平台兼容性遵守根目录 `AGENTS.md` 的「平台兼容性开发规范」，UI 规范遵守 [docs/desktop-skin-plan.md](../desktop-skin-plan.md)，危险操作分级遵守 Roadmap §46。
> 与既有系列的关系：用户反馈快修走 [docs/tasks-fix/](../tasks-fix/README.md)（F-XX），分析报告批次走 [docs/tasks-project-analysis-fix/](../tasks-project-analysis-fix/README.md)（PAF-XX）；本系列专注 **git 功能使用体验** 的系统性改进（含文档中已记录未落地的条目）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 修复中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |
| 💬 | 仅讨论（不排期） |

## 总体进度

- 任务总数：**20**
- 已完成：**4** · 修复中：**0** · 未开始：**15** · 仅讨论：**1**

---

## 任务索引

| 编号 | 问题 | 优先级 | 状态 | 文档 |
|---|---|---|---|---|
| GF-01 | 主界面搜索框失效：绑定了 `searchQuery` 但全文无任何过滤逻辑，用户输入零响应 | P0 | ✅ | [GF-01-changes-search-box-dead.md](./GF-01-changes-search-box-dead.md) |
| GF-02 | 批量操作失败零反馈：批量 Pull 内层 catch 静默吞失败、watcher 启停失败不提示 | P0 | ⬜ | [GF-02-batch-op-failure-silent.md](./GF-02-batch-op-failure-silent.md) |
| GF-03 | Ctrl+Shift+D 死键 + Diff/冲突解决器从命令面板消失（nav:false 路由被注册表过滤） | P1 | ⬜ | [GF-03-diff-shortcut-dead-command-palette-missing.md](./GF-03-diff-shortcut-dead-command-palette-missing.md) |
| GF-04 | Tags 只读孤岛：分支管理页能看到标签但完全不能操作（后端也无 tag 变更命令） | P1 | ✅ | [GF-04-tags-readonly.md](./GF-04-tags-readonly.md) |
| GF-05 | git 关键路径原生控件治理：冲突解决 RESULT 编辑器裸 textarea、hunk 暂存裸 button | P1 | ⬜ | [GF-05-native-controls-git-paths.md](./GF-05-native-controls-git-paths.md) |
| GF-06 | 过期文案误导用户：GitGraph/BranchManager 仍写「三方解决器随 T-16 提供」（T-16 早已交付） | P2 | ⬜ | [GF-06-stale-t16-copy.md](./GF-06-stale-t16-copy.md) |
| GF-07 | 单仓网络操作无进度无取消 + `push_branch` 同步命令阻塞 IPC 线程（与批次操作体验割裂） | P1 | ✅ | [GF-07-single-repo-netop-no-progress.md](./GF-07-single-repo-netop-no-progress.md) |
| GF-08 | Git 认证失败无可行动引导：`AppError::Git` 无 details/suggestedActions，"Authentication failed" 原样透出 | P1 | ✅ | [GF-08-git-auth-failure-no-guidance.md](./GF-08-git-auth-failure-no-guidance.md) |
| GF-09 | Ignore diff 选项静默禁掉行级暂存：已勾选的行选择无声丢失，仅一行小 tag 提示 | P1 | ⬜ | [GF-09-ignore-option-kills-staging.md](./GF-09-ignore-option-kills-staging.md) |
| GF-10 | Workspace Stash 多仓串行执行且无进度无取消（全应用唯一「黑屏等待」的写操作） | P2 | ⬜ | [GF-10-workspace-stash-serial-no-progress.md](./GF-10-workspace-stash-serial-no-progress.md) |
| GF-11 | git 长列表无虚拟滚动 + GitGraph「加载更多」全量重取（O(n²)），千级提交/文件卡顿 | P1 | ⬜ | [GF-11-git-lists-virtual-scroll.md](./GF-11-git-lists-virtual-scroll.md) |
| GF-12 | SmartMergeDialog 丢失冲突语义：`baseOid` 声明未用、`conflictType` 硬编码 both-modified（deleted-by-us/them 分支永不触发） | P2 | ⬜ | [GF-12-smart-merge-dialog-semantics.md](./GF-12-smart-merge-dialog-semantics.md) |
| GF-13 | git 小缺陷集合：Reflog 200 条上限无提示、分支条只显示 10 个、pick_continue 丢原作者（2026-09-24 核验：原「远程分支误判」子项已被 PAF-26 修复，撤销） | P2 | ⬜ | [GF-13-git-small-defects.md](./GF-13-git-small-defects.md) |
| GF-14 | git 死代码清理：RepositoryList `v-if="false"` 选择器死块、sync_fetch/pull/push 死 wrapper、三个无引用仓库组件 | P2 | ⬜ | [GF-14-git-dead-code-cleanup.md](./GF-14-git-dead-code-cleanup.md) |
| GF-15 | 批量 Pull 分叉后无策略跟进：dry-run 已能识别分叉仓库，失败后缺「批量选 merge/rebase」动作 | P1 | ⬜ | [GF-15-batch-pull-fork-followup.md](./GF-15-batch-pull-fork-followup.md) |
| GF-16 | Undo 覆盖面窄 + 冲突解决日志刷屏：stash drop/clear、merge abort、cherry-pick 等不入操作日志且不可撤销 | P1 | ⬜ | [GF-16-undo-coverage-conflict-log-spam.md](./GF-16-undo-coverage-conflict-log-spam.md) |
| GF-17 | 破坏性单仓操作无结构化预演：merge/rebase/reset 仅靠 UI 文案确认，未复用 batch_dry_run 成熟模式 | P2 | ⬜ | [GF-17-destructive-op-preview.md](./GF-17-destructive-op-preview.md) |
| GF-18 | 命令面板/快捷键未覆盖已有 git 操作：cherry-pick/merge/rebase/stash/worktree/repo-tools 均无命令注册 | P2 | ⬜ | [GF-18-command-palette-git-ops.md](./GF-18-command-palette-git-ops.md) |
| GF-19 | StatusBar 分支槽位空占位：`currentBranch = ref(null)`，Git 视图不显示当前分支（desktop-skin-plan 已规定） | P2 | ⬜ | [GF-19-statusbar-branch-slot.md](./GF-19-statusbar-branch-slot.md) |
| GF-20 | git 功能增强规划待排期：Blame 视图 / Interactive Rebase UI / Remote 管理 / Binary diff / Git 配置 UI / 主题三档切换 | 💬 | ⬜ | [GF-20-git-feature-roadmap.md](./GF-20-git-feature-roadmap.md) |

---

## 维护规范

1. 修复任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 本系列任务多源于静态分析而非运行时复现：**动手前必须先按「问题描述」复现**（能写失败测试的先写失败测试），复现结论写回任务文档「定位线索」。
3. 完成修复需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 根因 + 修法 + 验证命令）。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因；💬 讨论项不参与状态流转，结论明确后可转为正式任务。
5. 一个任务尽量一次完成；若牵出独立新问题，新增 GF-XX 文档并同步本表，不要在原文档里无限扩张范围。
6. 修复中涉及平台差异（路径 / 进程 / 可执行文件检测）时，先对照根目录 `AGENTS.md` 的「平台兼容性开发规范」；涉及同步 Tauri 命令线程模型的，对照 AGENTS.md「Tauri 命令线程模型硬规则（F-43）」。
7. 批次执行流程（任务选择、静态证据复现、依赖顺序、双处进度同步）见 `.agents/skills/gitworkspace-git-fix/SKILL.md`。
