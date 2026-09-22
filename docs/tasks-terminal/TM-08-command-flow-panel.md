# TM-08 命令流任务面板 + 退役 TM-06 字符串启动 + 终端 tab 应用名命名与装依赖/构建输出镜像

> 来源：用户反馈（2026-09-21）四联动改造：任务面板改命令流事件视图；退役 TM-06
> 「LaunchPlan 降级 shell 字符串写入 PTY」链路；终端 tab 以应用名命名；装依赖/
> 构建输出镜像到终端面板。登记为 TM-08（TM-01~TM-07 均已完成）。

## 背景

1. **任务面板鸡肋**：`src/views/TaskPanel.vue` 是「每任务一卡片」的状态快照列表，
   批量操作 N 个 repo 时扫读/定位失败项效率低；底部「Git 命令输出」区
   （监听 `git_command_result`，事后展示、截断 80px）与终端面板已有的
   `__git_console__` 流式镜像 tab 功能重叠且更弱。改为 **时间序事件流（命令流）**。
2. **TM-06 降级链路的代价**：「在终端中启动 runtime」把结构化 LaunchPlan 降级成
   shell 字符串写入用户 PTY（`runtime_start_in_terminal`），由此产生 F-44
   （PowerShell 行首引号 `&`）、F-45（裸 token 按字符集加引号）、F-51
   （chcp 65001 前缀）一整轮 shell 适配坑。结构化执行路径本已存在
   （`launch_command` 直接 spawn argv + env），字符串链路退役；「点了启动就在
   终端面板里看这个应用跑」的操作习惯保留（结构化启动 + 聚焦应用名 tab）。
3. **终端 tab 无法区分**：终端启动 tab 标题硬编码 `"Terminal"`。
4. **装依赖/构建看不到过程**：`node_install_output` 前端零消费；构建只有阶段事件
   （`runtime_build_progress`，payload 仅 `stage`），逐行输出只进 RingTail/日志。

## 目标与范围

### A. 任务面板 → 命令流事件视图（前端）

- `src/stores/task.ts`：`tasks` 增加 **append-only 事件日志**（时间戳、repo、
  taskType、状态迁移、耗时、error、batchId，上限 200 条）；保留当前状态索引供
  计数/取消/`waitForTasks` 使用；**API 语义不变**（`waitForTasks`/`cancelTask`/
  `clearFinished`/`loadActiveTasks` 调用方行为不变）。
- 删除「有运行中任务就自动弹面板」；改为收到失败/取消事件时 `showPanel()`。
- `task_progress` 监听上提到 **App 级挂载**（`src/App.vue` 调 `useTaskProgress()`）。
- 后端 `TaskManager::submit`（`src-tauri/src/task/manager.rs`）在任务**入队时**
  补发 `task_progress`（queued 事件），命令流首行即「排队中」。
- `src/views/TaskPanel.vue`：卡片列表改为事件流行：单行
  `时间 | 类型 badge | repo | 状态/耗时`；失败行红色、可展开完整 error；点击行
  跳转仓库视图（路由 `query.repo` 既有模式）；保留 batch 父行 + 可展开子行
  （按首个子事件时间插入时间线）；queued 取消按钮、「清除已完成」保留；删除
  git-console 区（与 Git Console tab 重复）。
- `src/types/task.ts`：补 `TaskEventEntry` 事件条目类型。
- `src/components/shell/StatusBar.vue`：存在失败任务时红色标识，点击开合不变。

### B. 退役 TM-06 字符串启动路径（Rust + 前端）

删除（调用方均已核实仅本流程使用）：

- `commands/terminal.rs::runtime_start_in_terminal` / `assemble_command_for_shell` /
  `utf8_console_prefix` / `is_sensitive_env` 及全部相关测试；`lib.rs` 注销注册。
- `runtime/launch/launcher.rs::plan_shell_command` / `shell_quote_path` /
  `shell_quote_arg` / `arg_needs_quoting` 及相关测试（保留 `launch_command` /
  `plan_preview` / `plan_working_dir`）。
- `launch/manager/start.rs::compute_launch_plan`、`commands/runtime.rs::
  runtime_compute_launch_preview`（整链仅服务终端启动预览）。
- `runtime_{register,unregister,stop}_terminal_process` 三个 IPC + service 方法
  （`queries.rs` 的 `register_terminal_process` / `unregister_terminal_process` /
  `get_terminal_session_id`）与 store 层 `insert_terminal_process` /
  `find_by_terminal_session`。**只删代码不删表**：`runtime_processes.
  terminal_session_id` 列与结构体字段保留（v23 schema 不动，无迁移）。
- 前端：`api/terminal.ts::runtimeStartInTerminal`、`stores/terminal.ts::
  launchInTerminal`（含 `launchedInTerminal` 会话标记与 exit 注销逻辑）、
  `api/runtime.ts` 四个 terminal-process 封装、`TerminalPanel.vue` 降级提示条。
- `process/pty.rs::shell_kind`/`ShellKind` 无其他使用方，一并删除并更新注释。
- `RuntimeDashboard.vue`：「在终端中启动」入口保留、改写为**结构化启动**
  （同 managed_start 的 `store.start` 服务端路径）+ 打开终端面板聚焦
  `__runtime_<应用名>` tab；`onStop` 移除终端分支。

### C. 终端 tab 应用名命名

- tab id 前缀：运行时 `__runtime_<name>`、装依赖 `__install_<name>`、
  构建 `__build_<name>`；标题：`<应用名>` / `<应用名> · 装依赖` /
  `<应用名> · 构建`。
- 托管启动与改写后的终端启动均在启动时**预创建并聚焦**对应 tab；同一应用重复
  触发聚焦已有 tab 不新开。
- `isRealShellSession` 排除全部镜像前缀；XtermView 对镜像 tab 不注册 onData
  （只读输出视图，不提供输入）。

### D. 装依赖 / 构建输出镜像（结构化执行 + 输出镜像，不进 PTY）

- 装依赖：terminal store 监听 `node_install_output`（后端已逐行发射），按
  taskId→应用名映射（RuntimeDashboard 提交任务时绑定）写入 `__install_<name>`
  tab；N-08 确认闸门保留。
- 构建：后端新增 `runtime_build_output` 事件（payload `{runtimeName, stream,
  line}`，命名遵守 F-15）；`RuntimeService::run_build`（`service/operations.rs`）
  的 sink 由纯 RingTail 换成「RingTail + 逐行发射」组合 sink（pipeline 层已脱敏，
  发射行无秘密；emitter 经 `self.emitter`，与既有事件同一 seam）。
- 任务终态时在对应 tab 追加一行结果摘要（成功/失败 + 错误摘要）；任务面板事件流
  与终端镜像双渠道数据同源（同取 task_progress / 后端事件）。

## 验收标准

1. 多仓库批量 fetch/pull：面板呈现时间序事件流；全部成功不自动弹窗；有失败自动
   展开并可见失败行（红色）。
2. 点击失败行跳转对应仓库视图；排队任务可取消；清除已完成正常。
3. 终端面板 Git Console tab 仍流式显示 `git_op_output`。
4. 托管启动 / 改写后的终端启动：终端面板打开，tab 名为应用名；重复启动聚焦已有
   tab。
5. 「装依赖」：确认闸门仍在；出现 `<应用名> · 装依赖` tab 并滚动输出。
6. 「构建」：出现 `<应用名> · 构建` tab 并滚动输出；失败时 tab 内有错误摘要。
7. 全仓 grep 零残留：`runtime_start_in_terminal` / `plan_shell_command` /
   `assemble_command_for_shell` / `launchInTerminal` / `runtimeStartInTerminal`。
8. `GW_TEST_MANIFEST=1 cargo test --lib`、`pnpm build`、`pnpm tauri build` 通过；
   `gitnexus detect_changes()` 范围符合预期。
9. AGENTS.md F-44/F-45/F-51 规则收缩；TM-06 spec 标记退役。

## 影响分析（GitNexus impact，2026-09-21）

| 符号 | 上游 | 风险 |
|---|---|---|
| `runtime_start_in_terminal` | 0（IPC 命令，前端唯一调用方随删） | LOW |
| `assemble_command_for_shell` | 5：本体 + 4 测试（同批删） | MEDIUM |
| `plan_shell_command` | 5：compute_launch_plan×2 + 4 测试（compute_launch_plan 唯一消费者 runtime_compute_launch_preview 唯一前端调用方 RuntimeDashboard 均随删） | MEDIUM |
| `RuntimeTaskHandler::execute` | 仅 RuntimeService 实现；exec_build→run_build 换 sink（不改 execute_build 签名） | LOW-MEDIUM |
| `execute_build` | 签名不变；sink 实现在调用方（operations.rs run_build）替换 | MEDIUM（新增事件发射，spawn_blocking 线程内经 emitter seam，与 node_install_output 同模式） |
| `launchInTerminal`（前端） | 唯一调用方 RuntimeDashboard:1280 | LOW |
| `updateTaskProgress`（前端） | useTaskProgress 监听 + StatusBar 计数；API 语义不变 | LOW |

DB：无表结构变更（仅删访问 terminal_session_id 的代码路径，列与字段保留）。

## 进度

| 日期 | 状态 | 记录 |
|---|---|---|
| 2026-09-21 | 🟦 | 任务登记：影响分析完成（见上表），开始改造 |
| 2026-09-22 | 🟦 | A 批（6e2c6ff）：任务面板改命令流事件视图 + App 级监听 + StatusBar 失败标识；后端 submit 补发 queued 事件 |
| 2026-09-22 | 🟦 | B 批（a3400fa）：退役 TM-06 字符串启动路径（terminal.rs / launcher.rs / start.rs / store.rs / commands/runtime.rs / queries.rs / pty.rs shell_kind / lib.rs / api 层） |
| 2026-09-22 | 🟦 | C+D 批（f6d1f12）：终端 tab 应用名命名（__runtime_/__install_/__build_ 前缀 + focusMirrorTab）+ 装依赖/构建输出镜像（后端 runtime_build_output 事件 + EmittingBuildSink；前端 node_install_output/buildOutput 消费 + bindInstallTask）+ ipc_golden 注册 TaskEventEntry |
| 2026-09-22 | ✅ | E 批：文档同步（AGENTS.md F-44/F-45/F-51 规则收缩；TM-06 spec 标记退役；terminal-feature-plan §4.5/§6）；cargo test --lib（失败集与基线一致，无回归）、pnpm build、gitnexus detect_changes 通过，4 批提交完成 |
