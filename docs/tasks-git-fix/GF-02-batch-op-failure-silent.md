# GF-02 批量操作失败零反馈（批量 Pull 静默吞失败、watcher 启停失败不提示）

> 状态：✅ 已完成（2026-09-24；GUI 实机回归：验收 1/2 的运行时复现待主智能体/用户
> 执行，静态验证与 pnpm build 已通过，见「进度」）
> 优先级：P0（用户看到「3 个仓库 Pull 完成」而不知道有 2 个失败了——信任受损型缺陷）
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

两处「操作失败但用户看不到」的模式：

1. **批量 Pull**：`handlePull` 内层 `catch {}` 注释写着「Individual repo failure doesn't stop
   the batch」，但失败仓库既不进 `successCount` 也不进冲突队列——最终只 toast
   「N 个仓库 Pull 完成」，失败的仓库凭空消失。
2. **文件监听启停**：`startFileWatcher` 失败只 `console.error`；
   `toggleWatcher` 的三元失败分支不弹任何提示——用户点了「启动监听」没有任何反馈。

## 定位线索（证据）

> 复现核验（2026-09-24，GF-01 等任务改过该文件，行号已漂移；结论：**漂移后仍成立**）：

- 批量 Pull 静默 catch：修复前 `src/views/RepositoryList.vue:2492-2494`
  （`catch {` 2492，注释 2493；任务文档原记 2455-2457，漂移 +37）。
- watcher 启动失败仅 console.error：修复前 `src/views/RepositoryList.vue:2597-2599`
  （console.error 在 2598；原记 2560-2562，漂移 +38）。
- toggleWatcher 失败分支无 toast：修复前 `src/views/RepositoryList.vue:2611-2616`
  （`toggleWatcher` 2602 起；else 分支只 `await startFileWatcher()` 后判
  `watcherActive`，无 else；原记 2575-2578，漂移 +36）。
- 批量操作入队路径（任务面板已有）：`git_ops.rs` 的 `batch_pull/batch_push/batch_fetch`
  （`src-tauri/src/commands/git_ops.rs:51/73/95`，原记 43/65/87 亦漂移），失败时
  task 状态带 error 字符串——**后端有数据，前端没有汇总展示**。
- 前端等结果机制（修复前已存在，本次复用未新造）：`taskStore.waitForTasks`
  （`src/stores/task.ts:41`，轮询 `list_active_tasks`；后端终态任务保留 30s，
  `task.rs:29-33`）+ `task_progress` 事件经 `useTaskProgress`（App 级）喂
  `taskStore.tasks`；`TaskPanel.vue` 直接用 `classifyGitErrorText` 渲染失败行
  引导（GF-08）。任务面板打开方式：`taskStore.showPanel()`。

## 修复范围 checklist

- [x] 1. 批量 Pull/Push/Fetch 收口时汇总失败清单（仓库名 + 简短原因），toast 明示，并提供「查看任务面板」动作。
- [x] 2. watcher 启动/停止失败：`message.error` 展示错误原因。
- [x] 3. 检查其余批量入口（批量提交、批量 branch op、批量 add/restore）是否有同样模式，有则一并收口。

## 不做（范围控制）

- 不改变「单仓失败不中断批次」的语义（这是合理设计，缺的是反馈）。
- 不重构任务队列错误上报链路（后端 error 字符串已有）。

## 验收标准

1. 构造一个远程不可达的仓库混入批量 Pull：完成后 toast 明确列出失败仓库与原因，成功数不含它。
2. watcher 启动失败（如制造一个不可监听路径）时有错误提示，不再静默。
3. 全成功场景提示与当前一致，无回归。

## 修复说明（2026-09-24）

**机制选择**：失败数据不新造轮询/监听。批量入队类（fetch/push/commit/branch op/
add/restore）统一走既有 `taskStore.waitForTasks`（轮询 `list_active_tasks`）等
终态，再从 `taskStore.tasks`（`task_progress` 事件喂养 + 后端 30s 终态保留
窗口）按提交的 taskIds 收 `failed`（error 字符串）/`partialSuccess`/`cancelled`；
批量 Pull 走仓内 `smartPull` 循环，直接把 catch 到的错误记入清单。简短原因
统一经 `errMsg`（GF-08 的 Git 错误引导）取首行截断，与 TaskPanel/GF-08 展示
同源。

**UI 选择**：naive-ui message 的 content 渲染函数把失败清单（标题「X：N 成功，
M 失败」+「仓库名：原因」≤5 条 + 计数折叠）渲染进 error toast（10s、可关闭），
附「查看任务面板」按钮（`taskStore.showPanel()`，与 ManifestView/ChangeSetView
一致）。选 toast 而非 n-modal：失败通常 1~3 条、toast 可承载且不打断当前操作；
完整明细（每行的 GF-08 分类 chip、重试）在任务面板。注意用 content 渲染函数
而非 message 的 `render` 选项——后者会整块替换默认卡片（丢 error 图标/样式/
关闭按钮）。toast 挂在全局 MessageProvider 下，SFC scoped 样式不生效，故清单
样式用内联 + `--gw-*` tokens。

**验收对照**：1/2 为运行时行为，本次以静态核验（代码路径走查）+ `pnpm build`
（vue-tsc）验证，GUI 实机回归（远程不可达仓库混入批量 Pull、制造启动失败）
由主智能体/用户后续执行；3 由代码保证——全成功路径的成功 toast 文案一律保持
原样（`已提交 N 个 … 任务` / `已暂存/回退/丢弃 …`），仅失败时才追加汇总
error toast。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（静态证据见上）。待复现与修复。 |
| 2026-09-24 | 完成修复（⬜→✅）。根因：批量 Pull 内层 `catch {}`（原 2492-2494）吞掉单仓失败；watcher 启动失败只 console.error（原 2598）、toggleWatcher 启动失败分支无提示（原 2611-2616）；其余批量入口（push/fetch/commit/branch op/add/restore/dry-run 执行）入队后仅「已提交 N 个任务」toast，队列内 per-repo 失败无汇总。修法：统一收口助手 `waitBatchAndReport`（复用 taskStore.waitForTasks 既有轮询等终态 → collectBatchFailures 按 taskIds 收 failed/partial/cancelled）→ `showBatchFailureToast`（naive-ui message render 渲染「仓库名：原因」清单 ≤5 条 + 计数折叠 + 「查看任务面板」按钮 `taskStore.showPanel()`，10s 可关）；简短原因经 GF-08 `errMsg` 取首行截断；handlePull 改为 catch 内记清单，收口时一条汇总 error toast 替代（成功场景 toast 文案不变）；startFileWatcher catch 改 `message.error` 并经 toggleWatcher 反馈（空变更列表 warning）。impact：handlePull/startFileWatcher 等 9 个符号 upstream 全 LOW；detect_changes 显示 touched 19 个符号（多为插入 185 行引起的行移映射噪声），受影响 process 均为 onContextmenuSelect 族（预期内）。验证：`pnpm build`（vue-tsc --noEmit + vite build）通过。README 总表按本次指令未动。GUI 实机回归（远程不可达仓库混入批量 Pull / watcher 启动失败）待主智能体或用户执行。 |
