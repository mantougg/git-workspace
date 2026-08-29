---
name: gitworkspace-ai-dev
description: GitWorkspace AI Assistant 任务开发流程：如何读 docs/tasks-ai/ 文档（总索引/全局约束/任务spec）开始与继续 AI（AI-XX）任务开发、并同步进度。
---

# GitWorkspace AI Assistant 任务开发流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-ai/` 的任务文档**开始开发**或**继续开发**某个 AI（AI-XX）任务。

AI Assistant 是应用智能层（设计来源：`docs/ai-assistant-design.md`），与 Git Workspace（T-XX）、Runtime Workspace（R-XX）同库共生：**基础设施复用而非重建**——SQLite（T-03）、Diff（T-04）、Task Queue（T-05）、错误/日志/Secret（T-08）、Runtime 日志与结构化错误（R-11/R-14）、IPC/Task 集成（R-12）。

## 文档地图

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/ai-assistant-design.md` | 设计文档：产品与技术约束的**单一事实来源**（§ 号被各 spec 引用） | 对设计依据有疑问、spec 与它冲突时 |
| `docs/tasks-ai/README.md` | 总索引：12 个任务的阶段/优先级/状态/依赖总表 + 依赖链 + 第一阶段口径 + 维护规范 | 选任务、核对状态、同步进度时 |
| `docs/tasks-ai/00-全局开发约束.md` | AI 横切硬约束（AI as Assistant / 统一调用链 / 凭证安全 / Secret 与 Preview / 只读工具 / 缓存隔离 / 错误模型 / 异步性能 / 跨平台 / 审计 / 复用边界） | 任何 AI 任务开发前**必读** |
| `docs/tasks-ai/AI-XX-*.md` | 任务 spec：目标 / 需求范围 / 架构性能注意点 / 验收标准 / 进度 | 开发目标任务时 |

涉及 Git 联动（AI-07~09）或 Runtime 联动（AI-06）时，`docs/tasks/00-全局开发约束.md`、`docs/tasks-runtime/00-全局开发约束.md` 与根 `AGENTS.md` 平台规范一并生效（任务 spec 顶部「开发前必读」会标注）。

## 关键边界（贯穿所有 AI 任务）

1. **AI as Assistant**：只建议/解释/分析；写操作一律「Proposal → 预览 → 用户确认 → 现有命令/任务队列执行」，AI 层不持有 `Repository` 句柄、不 spawn 子进程。
2. **Preview 是硬要求**：未确认不得联网；Secret 复用 T-08，最终内容生成后扫描 + 脱敏后二次扫描。
3. **凭证不落盘**：API Key 只存 OS Credential Store，不进日志/错误/LocalStorage/进程命令行。
4. **第一期只读**：工具白名单全只读，单次请求工具调用上限 8 次，无自主循环 Agent。
5. **Offline First**：AI 不可达不影响 Git/Runtime 核心功能；不能假装使用了本地模型。

## 第一阶段口径

- 第一阶段 = **Phase A + Phase B**（AI-01 ~ AI-06）：Foundation（Provider/凭证/Gateway/上下文/会话）→ 工具注册表 → Runtime 只读排障。
- Phase A 是全部后续任务的前置；AI-06 验证通过后再启动 Git Assistant 系列（AI-07 ~ AI-09）。
- T-25/T-26/T-27/R-26 是产品场景占位 spec，实现由 AI-06/AI-08/AI-09 承载（对应关系见 README 总表末尾）。

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选一个「无依赖」或「依赖均已就绪」的任务；依赖含 T-XX/R-XX 时确认对应任务状态）。
2. 读 `README.md` 总表，确认该任务的状态、优先级、直接依赖。
3. 读 `00-全局开发约束.md`（必读，贯穿所有 AI 任务）。
4. 读目标任务文档顶部的「**开发前必读**」指针，按它列出的「直接依赖」加载依赖任务文档和设计文档对应章节——**只读这几份，不要全读**。
5. 通读目标任务文档，明确：目标、需求范围（checklist）、验收标准。
6. 把任务状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」章节），并在时间线追加一行「开始开发」。
7. 开始实现。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」章节：当前状态 + 时间线最后一条 + 子任务清单勾选情况。
2. 读 `README.md` 总表该任务行，核对两处状态一致（不一致时以任务文档为准，并修正 README）。
3. 从时间线最后一条记录恢复上下文，继续**未勾选的子任务**。

## 完成一个任务

1. 逐条核对「验收标准」，**全部满足**才算完成；安全类验收（无自动修改、无直接 shell 路径、凭证不落地）需代码走查确认并在时间线注明。
2. 运行相关测试/构建验证（`cargo test`、`cargo check`、`pnpm build` 等，按改动范围选择）；IPC 类型变更必须更新 golden-file 快照；凭证/网络相关测试环境不可用时 skip 并打印原因，不硬失败。
3. 更新任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）。
4. 同步更新 README 总表该任务状态 `→ ✅`，并更新「总体进度」计数。
5. 若该任务承载某个 T/R 场景（AI-06→R-26、AI-08→T-25/T-27、AI-09→T-26）：在对应 T/R spec 时间线追加「由 AI-XX 实现」并更新其状态与所属 README 总表。
6. 若存在依赖此任务的下游任务，提示用户可开始下游；AI-06 完成意味着第一阶段（AI-01~06）收尾。

## 必须遵守

- **全局约束优先**：`00-全局开发约束.md`（AI 特有）是硬约束，任务文档「架构/性能注意点」是叠加的特有约束；若冲突，在任务文档显式说明原因与边界。
- **进度与状态规则以 README 为准**：进度两处同步、状态流转的权威定义在 `README.md` 末尾「维护规范」；本 skill 各步骤按它执行，不另立规则。
- **设计文档是单一事实来源**：spec 与 `docs/ai-assistant-design.md` 冲突时，先改设计文档或在 spec 显式说明，不静默偏离。
- **代码落点**：后端 `src-tauri/src/ai/` + `src-tauri/src/commands/ai.rs`（IPC 薄适配），前端 `src/{api,components/ai,composables,stores,types,views}/`；AI 层只编排，不重复实现 Git/Runtime 领域逻辑。
- **前端规范**：新 UI 用 tokens 变量与骨架组件（desktop-skin 约定）；命令与快捷键走命令注册表，禁止视图内各自绑定。
- **基础设施复用边界**：Secret/日志复用 T-08、SQLite 复用 T-03、任务执行复用 T-05/T-24/T-34、Diff/Runtime 数据走现有服务——**禁止另起一套**（全局约束 §13 有对照表）。
