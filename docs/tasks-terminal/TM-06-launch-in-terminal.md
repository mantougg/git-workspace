# TM-06 在终端中启动（LaunchPlan.preview 入 PTY，完整模式 + 降级模式）— 已退役

> **退役说明（TM-08，2026-09-21）**：本任务的「LaunchPlan 降级成 shell 字符串
> 写入用户 PTY」链路已整体退役。原因：字符串链路把结构化 LaunchPlan 降级为
> shell 命令串，由此产生 F-44（PowerShell `&` 调用运算符）、F-45（裸 token
> 引号包裹）、F-51（chcp 65001 前缀）一整轮 shell 适配坑；而结构化执行路径
> （`launcher::launch_command` 直接 spawn argv + env + 管道）与 runtime 输出
> 镜像 tab 本已存在。
>
> **替代方案**：「在终端中启动」入口保留、改写为**结构化启动**（同
> managed_start 的服务端路径）+ 打开终端面板聚焦 `__runtime_<应用名>` tab；
> 装依赖/构建输出镜像到 `__install_/__build_<应用名>` tab。详见
> [TM-08-command-flow-panel.md](./TM-08-command-flow-panel.md)。
>
> 以下内容为历史存档，不再适用。

| 项 | 值 |
|---|---|
| 阶段 | 三期 · Runtime 终端化 |
| 优先级 | P2 |
| 状态 | ⏸️ 已退役（TM-08 起由结构化启动 + 输出镜像 tab 替代） |
| 依赖 | TM-05 |
| 对应方案 | §4.5 在终端中启动 |

## 目标

提供「在终端中启动」可选模式：优先使用缓存的 `LaunchPlan` 真实启动命令（完整模式），无缓存时降级提示。构建仍走原链路产出 `LaunchPlan`，随后打开一个可交互 Shell tab 并写入启动命令执行——进程真正运行在 PTY 中，支持 stdin 交互（如需要控制台输入的调试场景）。含脱敏闸门和 env 注入（平台感知）。

## 需求范围

### 后端

- [x] 新增 `runtime_start_in_terminal(command, cwd, env)`：打开 PTY 会话并写入启动命令
- [x] 新增 `runtime_get_launch_preview`：从进程管理器的 launch_cache 获取缓存的启动命令
- [x] env 注入方式按平台分支：unix `A=b C=d cmd` 前缀；Windows `set A=b && cmd`
- [x] **脱敏闸门**：env 含 SECRET/TOKEN/PASSWORD/API_KEY 等敏感关键词时拒绝该模式
- [x] 会话登记进 `TerminalManager`，应用退出随会话清理

### 前端

- [x] RuntimeDashboard 启动按钮旁新增「终端启动」按钮（降级模式入口）
- [x] 打开对应终端 tab 并聚焦；tab 标题关联 runtime 名
- [x] tab 内顶部固定降级提示条（⚠️ 降级模式：无健康检查/端口检测/日志落盘/AI 诊断）
- [x] 该模式启动的进程不进入既有 process manager

## 验收标准

- [x] 命令可通过 PTY 执行，Ctrl-C / 输入交互可用
- [x] 非降级模式：使用缓存的 LaunchPlan 真实启动命令（首次成功启动后可用）
- [x] 降级模式：无缓存时提示用户先正常启动一次
- [x] 含敏感 env 的配置被拒绝且提示可行动（脱敏闸门检测 SECRET/TOKEN/PASSWORD 等）
- [x] 关闭 tab 后进程树无残留；Dashboard 运行态标识不误报
- [x] `cargo check` + `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-08 开发完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 三期-2） |
| 2026-09-08 | 🟦 | 开始开发：后端 runtime_start_in_terminal 命令 + 前端入口 |
| 2026-09-08 | 🟦 | 后端 runtime_start_in_terminal 实现（PTY 打开 + 命令写入） |
| 2026-09-08 | 🟦 | env 注入（平台感知：unix 前缀 / Windows set 命令）+ 脱敏闸门（SECRET/TOKEN/PASSWORD 等） |
| 2026-09-08 | 🟦 | RuntimeDashboard「终端启动」按钮 + 降级提示条（⚠️ 无健康检查/端口检测/日志落盘） |
| 2026-09-08 | 🟦 | 非降级模式：后端 runtime_get_launch_preview（从 launch_cache 获取真实启动命令） |
| 2026-09-08 | ✅ | 开发完成：前端优先使用真实命令（完整模式），无缓存时降级提示，cargo check + pnpm build 通过 |
| 2026-09-08 | ✅ | 非降级模式集成 LaunchPlan（后端 runtime_get_launch_preview + 前端优先使用真实命令） |
| 2026-09-08 | ✅ | 验收标准全部更新（含非降级模式验收标准） |
| 2026-09-21 | ⏸️ | **已退役（TM-08）**：字符串降级链路易引发 F-44/F-45/F-51 一轮 shell 适配坑，结构化 spawn 路径本已存在；删除 runtime_start_in_terminal / assemble_command_for_shell / plan_shell_command 等，「在终端中启动」入口改写为结构化启动 + 聚焦 __runtime_<应用名> tab，装依赖/构建输出镜像到 __install_/__build_ tab。历史内容存档备查 |
