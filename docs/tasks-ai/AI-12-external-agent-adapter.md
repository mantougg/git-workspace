# AI-12 外部 Agent Adapter（MCP / CLI）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-05](./AI-05-tool-registry.md)（Tool Registry 是唯一工具来源）、[T-31](../tasks/T-31-command-palette.md)（命令面板/CLI 入口）、[T-32](../tasks/T-32-plugin-system.md)（插件系统，仅作边界参考）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §15。

| 项 | 值 |
|---|---|
| 阶段 | Phase E · 受控写与外部 Agent |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | AI-05, T-31, T-32 |
| 对应设计文档 | §15 外部 Agent 能力规划 |

## 目标

把内部 Tool Registry 暴露给外部 AI Agent（MCP / CLI，预留 HTTP/API），让外部 Agent 以受控方式使用 GitWorkspace 的发现、状态、Diff、Runtime、日志能力。**不维护第二套能力与安全边界**。

## 需求范围

- [ ] MCP Adapter：Tool Registry → MCP tools 的映射层（名称、Schema、角色白名单直接复用 AI-05 定义）
- [ ] CLI Adapter：相同工具的命令行入口（供脚本与外部 Agent 调用）
- [ ] 外部调用方身份与角色：外部 Agent 以独立角色接入，权限**不超过**对应内置角色
- [ ] 第一阶段外部能力只读（§15）：Workspace/Repository 发现、状态与 Diff 查询、Runtime 配置/Closure/进程/日志查询
- [ ] Build/Run 的 Action Proposal（依赖 AI-11）：外部执行类请求必须携带确认标记，GitWorkspace 重新执行权限、范围与安全校验；**不能因来自 MCP/CLI 就绕过 UI 确认规则**（§15）
- [ ] 外部调用审计：来源标识、工具名、参数 hash、结果大小进 `ai.log`
- [ ] 与 T-32 插件系统边界（§15）：AI Tool 不复用任意脚本插件；插件权限/沙箱/来源信任成熟前，不允许用户注册自定义 AI 工具

## 架构 / 性能注意点

- Adapter 是**纯映射层**：不含业务逻辑，不允许出现 Registry 之外的能力；新增工具只在 Registry 侧加一次，各 Adapter 自动获得。
- MCP server 生命周期随应用启停；端口/进程管理遵守全局平台规范（不硬编码、不依赖 shell 探测）。
- 外部请求同样经过 Secret 与结果大小上限管道（AI-05 工具定义里已声明的约束对 Adapter 一视同仁）。

## 验收标准

- [ ] 外部 Agent 不能绕过工具权限和确认机制（§18.2/§18.4：越权调用被拒、写操作缺确认标记被拒，均有测试断言）
- [ ] MCP/CLI 两个 Adapter 的工具清单与 Registry 一致（一致性测试）
- [ ] 外部调用全部只读（走查确认无写路径）
- [ ] 审计日志含来源标识，无敏感原文
- [ ] 用户取消、关闭窗口或网络断开时无残留未确认动作（§18.4）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Registry → MCP 映射层
- [ ] Registry → CLI 映射层
- [ ] 外部角色与确认标记校验
- [ ] 外部调用审计
- [ ] 单元/集成测试与安全走查
