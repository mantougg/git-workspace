# GF-10 Workspace Stash 多仓串行执行且无进度无取消

> 状态：✅ 已完成
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

Workspace Stash（工作区快照，保存/恢复跨仓库 stash）对每个仓库**串行**执行
`git stash`，且**无进度事件、无取消**。在几十个仓库的工作区里，这是全应用唯一一个
「黑屏等待」的写操作——其余批量写操作（fetch/pull/push/commit/stage/branch op）都走
任务队列，有 TaskPanel 进度、Console 镜像、可取消。

## 定位线索（证据）

> 2026-09-24 复现核验结论：**四条线索全部成立**（行号有漂移，已就地更正）。
> 另核验两点设计约束：worker 池为 8 workers（`lib.rs:172` `TaskManager::new(8, ...)`），
> 逐仓 TaskRequest 会变成 8 路并行——与「保持串行」的范围控制冲突，故采用
> **单任务整笔入队 + 体内逐仓串行 + 每仓后发进度事件**方案；前端
> `RepositoryList.vue:2312/2384` 直接 await 命令返回值，为不改该文件（并行任务在改），
> 命令保持原签名改为 `async`（入队后 await 运行结果），`src/api/workspaceStash.ts`
> 因此零改动。

- 串行循环：`src-tauri/src/core/workspace_stash.rs:106-173`（`stash_repos`，`for path in repo_paths` 在 `:118`）
- 命令层无进度：`src-tauri/src/commands/workspace_stash.rs:47`（save 调 `stash_repos`，前后仅 DB 操作）
- restore 同路：`src-tauri/src/commands/workspace_stash.rs:112`（`restore_workspace_stash` 调 `restore_items`）
- 对照：批量任务队列治理 `src-tauri/src/task/worker.rs`（进度 emit `:631`、cancel 检查点 `:151`/`:191`/`:447`，取消注册表 `task/manager.rs:381-425`）——文档原锚点 `:472-476` 已漂移至 console 文案区，机制仍在上述行
- 前端入口：`src/api/workspaceStash.ts` → `RepositoryList.vue:2303 saveWsStash / :2380 confirmWsStashRestore`（同步 await，无任务队列、无进度、无取消入口）

## 修复范围 checklist

- [x] 1. save/restore 改为入任务队列（新增 TaskType 或复用批量任务），TaskPanel 可见逐仓进度与结果。
      → 新增 `TaskType::WorkspaceStashSave` / `WorkspaceStashRestore`（models/task.rs），
      整笔一个任务入队；worker 体内逐仓串行执行（模型不变），每仓后发
      `workspace_stash_progress` 事件（task/worker.rs `run_ws_stash_save` /
      `run_ws_stash_restore`），前端 stores/task.ts 累积、TaskPanel「逐仓明细」
      展开渲染（状态 chip + 失败详情）。
- [x] 2. 支持取消；**必须定义部分完成的语义**（哪些仓已 stash / 哪些未动，失败时给出可恢复清单——恢复预检 `check_workspace_stash` 已有安全网）。
      → cancel flag 逐仓检查点（core `stash_repos_cancellable` /
      `restore_items_cancellable`）。语义：**save**——已 stash 的仓库照常写入
      `Workspace Stash #N` 记录（取消也可恢复），未处理的仓库报 `cancelled`
      （stash message 已在栈、可重跑，已 stash 仓会被识别为干净而跳过）；
      **restore**——已 apply 的保留改动，未处理的 stash 仍在栈中（重跑恢复即
      可，预检复把关）。worker 每条完成路径（含排队期取消）都登记
      `WorkspaceStashRunResult`（task_id → 部分完成清单），等待中的命令据此
      解析而不是盲等；TaskPanel 取消行给出「N 个已 stash / M 个未处理——下一步」
      提示。
- [x] 3. 保留现有安全预检：`check_workspace_stash`（分支不匹配需 `allow_branch_mismatch`）在入队前执行。
      → `restore_workspace_stash` 入队前跑 `check_restore` + `check_allows_apply`，
      无可应用仓库时直接返回逐仓 skipped（不再入队空跑）；执行时逐仓复检
      （stale 窗口安全网）与原逻辑一致。

## 不做（范围控制）

- 不改串行→并行的 git 执行模型（stash 跨仓无依赖，但并行会放大机器负载与错误面；先补可观测性，并行化另立任务评估）。
- 不动 DB 记录结构。

## 验收标准

1. 多仓（≥10）workspace stash 保存/恢复：TaskPanel 显示逐仓进度，耗时与当前串行版本持平即可，但等待期间界面可交互、可取消。
2. 中途取消：已完成的仓库状态正确（不半途损坏），给出后续处理提示。
3. `cargo test --lib`（GW_TEST_MANIFEST=1）通过；既有 workspace_stash 单测无回归。

> 验收结论（2026-09-24）：
> 1. ✅ 命令改 `async` 入队（F-43：宏 spawn 进全局 runtime，await 不占 IPC
>    回调线程），等待期间界面可交互；TaskPanel 逐仓明细（含进度序号 N/M）。
>    命令签名不变 → `RepositoryList.vue` / `src/api/workspaceStash.ts` 零改动
>    （后者确实不需要改）。
> 2. ✅ 取消语义见 checklist #2；单测覆盖：`cancel_mid_save_reports_untouched_repos`、
>    `cancel_mid_restore_leaves_stashes_on_stack`、
>    `ws_stash_save_run_cancel_after_first_repo_keeps_partial_record`。
> 3. ✅ `GW_TEST_MANIFEST=1 cargo test --lib`：980 passed / 15 failed——
>    15 个全部为**既有环境依赖失败**（real_maven×10、real_node_vite×1 计时
>    抖动（隔离重跑通过）、pty smoke、node workspace×2、pathutil 大小写），
>    已用 `git stash` 基线重跑逐一比对确认（14/15 基线同样失败，node_vite
>    隔离可通过）；workspace_stash 既有 3 个单测无回归，新增 13 个全过。
>    `pnpm build`（vue-tsc + vite）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（串行 for 循环 + 无进度事件，均已静态核验）。待复现与修复。 |
| 2026-09-24 | 复现核验完成：四条定位线索全部成立（行号漂移已更正，见上）。开始修复：方案为单任务整笔入队（TaskType::WorkspaceStashSave/Restore）+ worker 体内逐仓串行 + 每仓 `workspace_stash_progress` 事件 + cancel flag 逐仓检查点；save/restore 命令签名不变改 async，入队后 await 部分完成结果（RepositoryList.vue 零改动）。 |
| 2026-09-24 | 修复完成。根因：save/restore 是同步命令内 `for` 循环直接打 libgit2，既不走任务队列（无进度/Console/取消），同步执行又占 WebView IPC 回调线程（只改 async 不够，还得入队）。修法：`models/task.rs` 新增 `WorkspaceStashSave/Restore` 两个 TaskType（整笔一任务，payload 带 record_name/repo_paths 或 workspace_stash_id+allow_branch_mismatch）；`core/workspace_stash.rs` 拆出 `stash_repos_cancellable` / `restore_items_cancellable`（cancel flag 逐仓检查点 + `on_repo` 进度回调，纯函数 `WorkspaceStashRunSummary::from_outcomes/to_task_status/summary_line`、`check_allows_apply`、`cancelled_before_start`）+ 8 个新单测；`task/worker.rs` 新增 `run_ws_stash_save` / `run_ws_stash_restore`（可单测的任务体，5 个新单测）+ worker 分支 + 早取消也登记结果 + 状态按 rollup 派生 + 1h 长超时；`task/manager.rs` 加 `ws_stash_runs` 结果注册表与 `take_ws_stash_run`；`commands/workspace_stash.rs` save/restore 改 async：入队前预检 restore，入队后轮询取部分完成结果（150ms 轮询 / 1800s 超时 / 终态 10s 宽限）；`core/git_ops` 加防御分支；前端 `stores/task.ts` 累积 `workspace_stash_progress`、`useTaskProgress.ts` 加监听、`TaskPanel.vue` 新 badge/取消入口（running 也可取消）/逐仓明细/取消后续提示，`types/task.ts`+`types/workspaceStash.ts` 同步类型，golden 快照重生成。验证：`GW_TEST_MANIFEST=1 cargo test --lib` 980 passed/15 failed（15 个均为存量环境失败，stash 基线比对确认，node_vite 隔离重跑通过）、`cargo test --lib workspace_stash`+`task::worker` 22 全过、`pnpm build` 通过、`detect_changes` 影响面限于 workspace_stash/task 队列/TaskPanel。 |
