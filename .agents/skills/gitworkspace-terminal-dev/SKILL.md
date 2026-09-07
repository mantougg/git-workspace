---
name: gitworkspace-terminal-dev
description: GitWorkspace 内嵌终端任务流程：如何读 docs/tasks-terminal/ 文档（总索引/全局约束/任务spec）开始与继续 TM-XX 任务（内嵌终端：PTY 交互 Shell / Git 输出镜像 / Runtime 终端化）开发、并同步进度。
---

# GitWorkspace 内嵌终端任务流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-terminal/` 的任务文档**开始开发**或**继续开发**某个内嵌终端任务（TM-XX 编号）。

## 文档地图

内嵌终端任务已拆解在 `docs/tasks-terminal/` 下：

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/tasks-terminal/README.md` | 总索引：7 个 TM-XX 任务的阶段/优先级/状态/依赖总表 + 依赖链 + 维护规范 | 选任务、核对状态时 |
| `docs/tasks-terminal/00-全局开发约束.md` | 终端特有横切硬规则：PTY 字节流/base64 传输、ConPTY 边界、kill 复用、事件批量、安全闸门 | 任何 TM-XX 任务开发前**必读** |
| `docs/tasks-terminal/TM-XX-*.md` | 任务 spec：目标 / 需求范围 checklist / 验收标准 / 进度 | 开发目标任务时 |
| `docs/terminal-feature-plan.md` | **设计 spec（架构与契约基准）**：方案 C 决策记录、§4 架构设计（§4.2 IPC 契约是前后端联调唯一基准）、§5 平台细则 | 开发任何 TM-XX 任务前**必读对应章节** |

约束文档（按需加载，不重复读）：

- 平台兼容性（路径归一化 / find_in_path / process_group / CREATE_NO_WINDOW）→ 根目录 `AGENTS.md` 的「平台兼容性开发规范」
- 既有执行/日志链路现状（streaming / kill_tree / git_ops / LaunchPlan / runtime 事件）→ `terminal-feature-plan.md` §2 现状盘点表

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选一个「依赖均已 ✅」的任务）。
2. 读 `README.md` 总表，确认该任务的状态、优先级、直接依赖。
3. 读 `00-全局开发约束.md`（必读，贯穿所有任务）。
4. 读目标任务文档顶部的「**开发前必读**」指针，读 `docs/terminal-feature-plan.md` 对应章节——**架构/IPC 契约以 spec 为准，不要凭记忆自由发挥**。
5. 通读目标任务文档，明确：目标、需求范围（checklist）、验收标准。
6. 把任务状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」章节），并在时间线追加一行「开始开发」。
7. 开始实现。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」章节：当前状态 + 时间线最后一条 + 子任务清单勾选情况。
2. 读 `README.md` 总表该任务行，核对两处状态一致（不一致时以任务文档为准，并修正 README）。
3. 从时间线最后一条记录恢复上下文，继续**未勾选的子任务**。

## 完成一个任务

1. 逐条核对「验收标准」，**全部满足**才算完成；TM-03 需三平台冒烟 checklist 实测逐条勾选（本机不可达的平台在文档注明，由用户协助验证）。
2. 运行相关验证（`cargo check` / `cargo test` / `pnpm build`，按改动范围选择）。
3. 更新任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）。
4. 同步更新 README 总表该任务状态 `→ ✅`，并重算「总体进度」计数。
5. 若存在依赖此任务的下游任务，提示用户可开始下游。

## 必须遵守

- **spec 优先**：架构、IPC 契约（命令/事件名/payload）、交互基准以 `docs/terminal-feature-plan.md` 为准（尤其 §4.2）；发现 spec 需要调整时，先改 spec 再改代码，并在任务时间线注明。
- **进度两处同步**：状态流转与维护规则以 `docs/tasks-terminal/README.md` 末尾「维护规范」为准。
- **功能链路不变**：TM-04/TM-05 只加镜像与渲染层，git 任务追踪、操作日志、runtime 日志引擎/落盘/健康检查等既有链路不得改道或削弱（方案 §3.2 决策记录）。
- **代码落点**：后端 PTY 在 `src-tauri/src/process/pty.rs`、命令在 `src-tauri/src/commands/terminal.rs`；前端组件在 `src/components/terminal/`、store 在 `src/stores/terminal.ts`、api 在 `src/api/terminal.ts`；入口走 StatusBar 槽位 + `src/commands/` 命令注册表。
