# AI-12 外部 Agent Adapter（MCP / CLI）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-05](./AI-05-tool-registry.md)（Tool Registry 是唯一工具来源）、[T-31](../tasks/T-31-command-palette.md)（命令面板/CLI 入口）、[T-32](../tasks/T-32-plugin-system.md)（插件系统，仅作边界参考）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §15。

| 项 | 值 |
|---|---|
| 阶段 | Phase E · 受控写与外部 Agent |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-05, T-31, T-32 |
| 对应设计文档 | §15 外部 Agent 能力规划 |

## 目标

把内部 Tool Registry 暴露给外部 AI Agent（MCP / CLI，预留 HTTP/API），让外部 Agent 以受控方式使用 GitWorkspace 的发现、状态、Diff、Runtime、日志能力。**不维护第二套能力与安全边界**。

## 需求范围

- [x] MCP Adapter：Tool Registry → MCP tools 的映射层（名称、Schema、角色白名单直接复用 AI-05 定义）
- [x] CLI Adapter：相同工具的命令行入口（供脚本与外部 Agent 调用）
- [x] 外部调用方身份与角色：外部 Agent 以独立角色接入，权限**不超过**对应内置角色
- [x] 第一阶段外部能力只读（§15）：Workspace/Repository 发现、状态与 Diff 查询、Runtime 配置/Closure/进程/日志查询
- [x] Build/Run 的 Action Proposal（依赖 AI-11）：外部执行类请求必须携带确认标记，GitWorkspace 重新执行权限、范围与安全校验；**不能因来自 MCP/CLI 就绕过 UI 确认规则**（§15）
- [x] 外部调用审计：来源标识、工具名、参数 hash、结果大小进 `ai.log`
- [x] 与 T-32 插件系统边界（§15）：AI Tool 不复用任意脚本插件；插件权限/沙箱/来源信任成熟前，不允许用户注册自定义 AI 工具

## 架构 / 性能注意点

- Adapter 是**纯映射层**：不含业务逻辑，不允许出现 Registry 之外的能力；新增工具只在 Registry 侧加一次，各 Adapter 自动获得。
- MCP server 生命周期随应用启停；端口/进程管理遵守全局平台规范（不硬编码、不依赖 shell 探测）。
- 外部请求同样经过 Secret 与结果大小上限管道（AI-05 工具定义里已声明的约束对 Adapter 一视同仁）。

## 验收标准

- [x] 外部 Agent 不能绕过工具权限和确认机制（§18.2/§18.4：越权调用被拒、写操作缺确认标记被拒，均有测试断言）
- [x] MCP/CLI 两个 Adapter 的工具清单与 Registry 一致（一致性测试）
- [x] 外部调用全部只读（走查确认无写路径）
- [x] 审计日志含来源标识，无敏感原文
- [x] 用户取消、关闭窗口或网络断开时无残留未确认动作（§18.4）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31 完成开发与验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发。依赖分析：AI-05 ✅ 已就绪；T-32 在本 spec 中明确为「仅作边界参考」，其边界（AI Tool 不复用脚本插件、不开放用户自定义工具注册）作为约束遵守，不构成启动阻塞；T-31 的实际范围是 GUI 命令面板/快捷键/IDE 集成，本任务所需的「CLI 入口」是面向脚本与外部 Agent 的无头命令行通道，不依赖 T-31 的任何交付物（spec 表头对 T-31 的「CLI 入口」描述与设计文档 §15 的 CLI Adapter 是两条独立通道），故在依赖说明留痕后启动。实现决策：不引入 rmcp/HTTP 框架等新依赖（全局约束 §13 允许按需引入协议级 crate；第一阶段仅需 initialize/ping/tools.list/tools.call 四个方法，零依赖的极简 JSON-RPC over HTTP（127.0.0.1，tokio 已有）即可满足「Adapter 是纯映射层」，避免为映射层引入框架级依赖树）。 |
| 2026-08-31 | ✅ | 完成。新增 `ai/external/`：`mod.rs`（`ExternalAgent` 独立身份 + 授权管道：只读工具直放、执行类工具需 `_meta/--confirm` 确认标记、注册表以 ActionPlanner 上限角色复检白名单/Schema/范围/Secret/预算/超时；`ai.log` 审计含来源/工具/参数 hash/结果大小/错误码，无参数原文）、`mcp.rs`（JSON-RPC 2.0 ⇄ 外部调用纯映射：initialize/ping/tools.list/tools.call，工具执行错误走 `isError` 结果、协议错误走 JSON-RPC error）、`server.rs`（tokio 裸实现 HTTP/1.1 传输，仅 127.0.0.1，默认端口 39117 被占回退临时端口，discovery 文件随应用退出清理）、`cli.rs`（`git-workspace ai-tools list|endpoint|call`：list/endpoint 离线可用，call 中继到运行中的应用实例；Windows GUI 子系统下 AttachConsole 回挂父控制台）。工具清单与 Registry 由同一 manifest 函数生成（一致性测试断言 1:1 + 双向名称映射 + Schema 逐项相等）。验证：`cargo check`、`cargo test ai::external`（21 项）、`cargo test ai::`（190 项）、`cargo test ipc_golden`、`schema_snapshot` golden 不变、`pnpm build`（vue-tsc）、CLI 冒烟（list/endpoint/call 错误路径、非 ai-tools 参数回退 GUI 启动）。安全走查：MCP/CLI 均只暴露 Registry 工具且 Proposal 执行无外部入口（仍走 AI-11 UI 确认）；外部管道本身无写路径（域写入全部在注册表复检后由 AI-11 既有管线执行）；注册表既有范围守卫（workspace/repoPath/进程归属）对 Adapter 一视同仁；应用退出 abort 服务任务并清理 discovery 文件；T-32 边界遵守——Adapter 仅映射静态注册表，未提供任何用户自定义工具注册口。detect-changes 标记 high 系 `run` 引导函数重构为 `build()+run(callback)`（仅新增 Exit 钩子清理，初始化序列未变）。 |

### 子任务清单

- [x] Registry → MCP 映射层
- [x] Registry → CLI 映射层
- [x] 外部角色与确认标记校验
- [x] 外部调用审计
- [x] 单元/集成测试与安全走查
