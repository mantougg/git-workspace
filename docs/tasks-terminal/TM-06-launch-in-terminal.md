# TM-06 在终端中启动（LaunchPlan.preview 入 PTY，降级模式）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.5（TM-06 部分）/ §8（风险：secret 泄露）+ [00-全局开发约束.md](./00-全局开发约束.md) §3；直接依赖：TM-05。关键现状：`LaunchPlan`（`src-tauri/src/runtime/build/mod.rs:140`）各变体均带可读 `preview` 命令串与 `env`/`working_dir`。

| 项 | 值 |
|---|---|
| 阶段 | 三期 · Runtime 终端化 |
| 优先级 | P2 |
| 状态 | 🟦 进行中 |
| 依赖 | TM-05 |
| 对应方案 | §4.5 在终端中启动 |

## 目标

提供「在终端中启动」可选模式：构建仍走原链路产出 `LaunchPlan`，随后打开一个可交互 Shell tab 并写入启动命令执行——进程真正运行在 PTY 中，支持 stdin 交互（如需要控制台输入的调试场景）。能力降级在 UI 明示。

## 需求范围

### 后端

- [ ] 新增 `runtime_start_in_terminal(runtimeName)`：复用构建链路产出 `LaunchPlan` → 校验可展示性 → 组装命令（`working_dir` + 必要 env 导出 + `preview`）→ 打开 PTY 会话并写入命令 + 回车
- [ ] env 注入方式按平台分支：unix `A=b C=d cmd` 前缀或 `export`；Windows cmd `set` / PowerShell `$env:`（参照 `user_script_command` 平台分支惯例）
- [ ] **脱敏闸门**：preview/env 含敏感项（复用日志引擎 `LogRedactor` 的判定）时拒绝该模式，返回可行动错误
- [ ] 会话登记进 `TerminalManager`（kind 标识 runtime 来源，关联 runtimeName/processId），应用退出随会话清理

### 前端

- [ ] RuntimeDashboard 启动按钮旁新增「在终端中启动」入口（下拉/二级按钮，不走任务队列）
- [ ] 打开对应终端 tab 并聚焦；tab 标题关联 runtime 名
- [ ] tab 内顶部固定降级提示条：此模式无健康检查 / 端口检测 / 日志落盘 / AI 诊断；停止 = 关闭会话（kill 进程树）
- [ ] 该模式启动的进程**不进入**既有 process manager（无 `GITWORKSPACE_PROCESS_ID` 登记），Dashboard 状态不误标为运行中——UI 上以 tab 存在态表达

## 验收标准

- [ ] Spring Boot / vite 项目可经该模式在终端中启动，日志颜色正确，Ctrl-C / 输入交互可用
- [ ] 含敏感 env 的配置被拒绝且提示可行动
- [ ] 关闭 tab 后进程树无残留；Dashboard 运行态标识不误报
- [ ] `cargo check` + `pnpm build` 通过；Windows / unix 命令组装分支经审查或实测

## 进度

### 状态

- 当前状态：🟦 进行中
- 最近更新：2026-09-08 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 三期-2） |
| 2026-09-08 | 🟦 | 开始开发：后端 runtime_start_in_terminal 命令 + 前端入口 |
