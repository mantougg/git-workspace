# TM-06 在终端中启动（LaunchPlan.preview 入 PTY，降级模式）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.5（TM-06 部分）/ §8（风险：secret 泄露）+ [00-全局开发约束.md](./00-全局开发约束.md) §3；直接依赖：TM-05。关键现状：`LaunchPlan`（`src-tauri/src/runtime/build/mod.rs:140`）各变体均带可读 `preview` 命令串与 `env`/`working_dir`。

| 项 | 值 |
|---|---|
| 阶段 | 三期 · Runtime 终端化 |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | TM-05 |
| 对应方案 | §4.5 在终端中启动 |

## 目标

提供「在终端中启动」可选模式：构建仍走原链路产出 `LaunchPlan`，随后打开一个可交互 Shell tab 并写入启动命令执行——进程真正运行在 PTY 中，支持 stdin 交互（如需要控制台输入的调试场景）。能力降级在 UI 明示。

## 需求范围

### 后端

- [x] 新增 `runtime_start_in_terminal(command, cwd, env)`：打开 PTY 会话并写入启动命令
- [x] env 注入方式按平台分支：unix `A=b C=d cmd` 前缀；Windows `set A=b && cmd`
- [x] **脱敏闸门**：env 含 SECRET/TOKEN/PASSWORD/API_KEY 等敏感关键词时拒绝该模式
- [x] 会话登记进 `TerminalManager`，应用退出随会话清理

### 前端

- [ ] RuntimeDashboard 启动按钮旁新增「在终端中启动」入口（下拉/二级按钮）— 后续优化
- [x] 打开对应终端 tab 并聚焦；tab 标题关联 runtime 名
- [ ] tab 内顶部固定降级提示条 — 后续优化
- [x] 该模式启动的进程不进入既有 process manager

## 验收标准

- [x] 命令可通过 PTY 执行，Ctrl-C / 输入交互可用
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
| 2026-09-08 | ✅ | 开发完成：后端命令 + 前端 API + store 方法，cargo check + pnpm build 通过 |
