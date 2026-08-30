# AI-10 统一 Assistant Drawer 与会话 UI

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-04](./AI-04-session-audit-cache.md)（会话/审计）、[AI-05](./AI-05-tool-registry.md)（工具）、[AI-06](./AI-06-runtime-assistant.md)（第一个场景验证）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §9.1、§9.2、§12.3、§12.4。

| 项 | 值 |
|---|---|
| 阶段 | Phase D · 统一应用助手 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-04, AI-05, AI-06 |
| 对应设计文档 | §9.1 产品形态、§9.2 角色模型、§12.3 Assistant Drawer、§12.4 未配置和离线状态 |

## 目标

提供全局统一的 **GitWorkspace Assistant** 入口（右侧 Drawer 或独立 Assistant View），把 AI-06 的单场景能力扩展为多角色、多轮会话的应用内助手；领域页面只提供上下文入口和专用快捷动作，不各自实现聊天状态。

## 需求范围

- [x] 全局右侧 Drawer（§12.3）：顶部（当前角色/模型/上下文范围）、中部（会话消息/工具读取摘要/建议卡片）、底部（输入框/发送/取消/清空上下文）；发送前 Preview Modal；失败状态提供「配置 AI / 重试 / 缩小范围 / 转到日志或 Runtime 页面」动作
- [x] 上下文带入入口（§9.1）：Workspace Dashboard、Repository/Changes/Diff、Conflict Resolver、Runtime Dashboard、Runtime Error Alert、Runtime Logs；界面展示当前作用域（如「当前工作区 / 3 个仓库 / Runtime gateway / 选中日志 86 行」）
- [x] 角色模型（§9.2）：七个受限角色；入口自动推断 + 手动切换，**自动推断结果必须在 UI 可见**
- [x] 多轮会话：会话重命名、清除、导出（§4.2 Phase D）；会话列表与消息加载复用 AI-04
- [x] 只读工具调用结果可视化：工具名、参数摘要、结果摘要、耗时，可展开查看来源
- [x] 自然语言查询应用状态（§3.2 场景 F）：第一期只调用只读工具，不自动操作
- [x] 命令注册表集成（desktop-skin 约定，设计文档未列，本任务补齐）：Drawer 打开/关闭注册为全局命令并分配快捷键，禁止视图内各自绑定
- [x] 降级状态（§12.4）：未配置/离线/请求失败的完整降级路径

## 架构 / 性能注意点

- 代码落点（§5.2）：`src/components/ai/{AssistantDrawer.vue, ConversationView.vue, AiSuggestionCard.vue}`、`src/composables/useAiAssistant.ts`、`src/stores/ai.ts`；骨架遵循 desktop-skin 约定（Panel/Toolbar、tokens 变量、不硬编码样式）。
- Drawer 是**全局唯一**会话状态持有者；不同 Workspace/Repository/Runtime 范围切换时不得串上下文（作用域为会话属性，切换需显式）。
- 流式渲染合帧（§16.1）；长会话列表分页、消息按需加载（AI-04 已支撑）。
- 打开 Drawer 不得触发全量 Repository 扫描（全局约束 §10）。

## 验收标准

- [x] §18.3 前端验收相关项：Drawer 在不同 Workspace/Runtime 范围间不串上下文；长响应流式渲染不阻塞现有页面；未配置/离线有明确降级
- [x] 六个上下文入口均能正确带入作用域并在 UI 展示
- [x] 角色自动推断可见、可手动覆盖；各角色工具调用不越权（复用 AI-05 矩阵测试）
- [x] 会话重命名/清除/导出可用；导出内容不含 Secret 原文
- [x] Drawer 打开/关闭快捷键走命令注册表（走查确认无视图内私有绑定）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 🟦 | 开始开发：依赖 AI-04 / AI-05 / AI-06 均已完成，接入全局 Drawer、会话状态与命令注册表。 |
| 2026-08-30 | ✅ | 完成：全局 Drawer、七角色自动推断/手动覆盖、会话重命名/删除/安全 Markdown 导出、会话与消息按需分页、只读工具卡片、命令注册表 `Ctrl+I`，以及 Dashboard/Changes/Diff/Conflict/Runtime/错误/日志上下文入口。验证：`cargo test --manifest-path src-tauri/Cargo.toml ai::session --lib`（22 passed）、`cargo test --manifest-path src-tauri/Cargo.toml ipc_golden --lib`（2 passed）、`pnpm build`。安全走查：新入口仅构建作用域和补充上下文，发送仍统一经 Preview → 确认 → Gateway；不同领域入口重置完整作用域，避免跨 Workspace/Repository/Runtime 串用；导出仅渲染结构化消息，未回放原始上下文或 Secret。 |

### 子任务清单

- [x] AssistantDrawer 骨架与全局会话 store
- [x] 角色模型与自动推断
- [x] 六处上下文入口接线
- [x] 多轮会话管理（重命名/清除/导出）
- [x] 工具调用结果可视化
- [x] 命令注册表集成与快捷键
- [x] 降级路径与前端验收
