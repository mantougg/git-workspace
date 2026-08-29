# GitWorkspace AI Assistant 设计文档

> 文档状态：设计草案
>
> 设计范围：AI 基础设施、应用内置 AI Assistant、Git AI 能力、Runtime AI 能力，以及未来供外部 Agent 调用的统一工具层。
>
> 本文不是某一个单独任务的实现 spec。它为 T-25/T-26/T-27、R-26 以及后续 AI 相关任务提供共同的产品和技术约束。

## 1. 需求背景

### 1.1 产品背景

GitWorkspace 的目标不是替代 IDEA，而是为大型多仓库项目提供轻量的 Git Workspace 和 Runtime Workspace。现有产品设计已经覆盖：

- 多 Repository 的状态、变更、分支、历史、批量操作；
- Maven 项目发现、依赖图、源码映射、Runtime Closure、构建、启动、日志和进程管理；
- 任务队列、DAG、Pipeline、文件监听、错误分类和操作日志。

这类产品天然积累了大量结构化上下文。用户遇到的问题通常不是“没有数据”，而是需要在多个页面和多个层次之间完成解释和判断：

- 哪些 Repository 实际发生了变更；
- 一次多仓库变更应该如何总结；
- 构建失败到底是 Maven、JDK、依赖、端口还是应用配置问题；
- 某个 Runtime 日志中的异常意味着什么；
- 一个冲突应该如何解决；
- 当前的操作是否会影响其他服务或工作区。

AI 适合承担解释、总结、分析、生成建议等工作，但不应绕过 GitWorkspace 已有的任务队列、安全确认、只读护栏和操作日志。

### 1.2 现有文档中已经存在的 AI 方向

产品文档已经提出两类 AI 能力：

1. **应用内的 AI Git Assistant**：Code Review、Commit Message、Commit Summary、PR Description、Conflict Resolution、Commit/File Explanation、Security Review 和 Bug Detection，见 [GitWorkspace Roadmap 第 22 节](./GitWorkspace 产品需求与技术架构 Roadmap.md#22-p1ai-git-assistant)。
2. **Runtime AI Assistant**：启动失败诊断、运行异常分析、配置建议，见 [R-26](./tasks-runtime/R-26-ai-runtime-assistant.md)。

产品信息架构也已经预留 `Settings/AI`，并把 AI Agent 描述为可以参与 `Clone → Discover → Build → Run → Test → Read Logs` 工作流的使用者，见 [Runtime 产品文档第 5.4 节](./大型企业项目轻量级开发运行工作台.md#54-ai-agent) 和第 7 节。

因此，本设计不是增加一个与现有产品割裂的聊天窗口，而是把已有的 AI 任务统一成一个可控的应用智能能力层。

### 1.3 当前实现问题

当前 AI 实现已经有 OpenAI-compatible API 的最小调用链，但仍处于原型阶段：

- `src/api/ai.ts` 直接暴露 `aiReview(repoPath, apiKey, apiUrl)`；
- `src-tauri/src/commands/ai.rs` 直接从命令参数接收 API Key；
- 模型名 `gpt-4o-mini` 在后端请求体中写死；
- API URL、模型、任务参数没有统一配置模型；
- AI 请求没有独立的 Provider、会话、上下文、工具和权限抽象；
- 当前代码审查主要依赖 diff，缺少多仓库上下文、Runtime 上下文和请求前 Preview；
- 已有 `ai_reviews`、`ai_tasks` 表，但没有形成可复用的会话、消息、请求审计和缓存模型；
- 前端目前没有 AI 设置页、会话状态、统一助手入口或能力降级提示。

这些问题如果由 T-25、T-26、T-27 和 R-26 分别解决，会导致多个任务重复实现 HTTP 调用、截断、Secret 检测、错误处理、缓存和确认流程。

## 2. 设计目标

### 2.1 总体目标

构建一个名为 **GitWorkspace Assistant** 的受控 AI 平台，使用户可以在 Git Workspace 和 Runtime Workspace 内：

- 用自然语言了解当前工作区、Repository、变更、Runtime 和日志；
- 获得代码审查、提交信息、冲突解决、PR 描述和 Runtime 排障建议；
- 使用用户选择的云端或本地模型；
- 在请求发送前看到将要发送的内容，并排除或脱敏敏感内容；
- 让 AI 通过类型化、可审计的工具读取应用上下文；
- 在涉及写操作时看到结构化变更提案，明确确认后才执行；
- 在 AI 未配置、网络不可用或请求失败时继续使用全部 Git/Runtime 核心功能。

### 2.2 设计原则

#### AI as Assistant

AI 只负责建议、解释、分析和生成。默认不自动：

- 修改用户源码、`pom.xml` 或其他项目文件；
- 修改 Git 分支、索引、提交、Push、Merge 或 Rebase 状态；
- 执行任意 shell、脚本或外部命令；
- 修改 Runtime 配置并落盘；
- 启动、停止、重启或 Kill 进程。

所有写操作必须经过“提案 → 预览 → 用户确认 → 现有命令/任务队列执行”。

#### 统一调用链

所有 AI 场景都复用同一条链路：

```text
业务入口
  ↓
上下文构建器 Context Builder
  ↓
Secret 检测 / 脱敏 / 文件排除
  ↓
发送 Preview
  ↓ 用户确认
AI Gateway
  ↓
Provider Adapter → Model
  ↓
结构化结果 / 建议 / Action Proposal
  ↓
会话、缓存、审计和 UI 展示
```

#### Offline First

AI 是增强能力。AI Provider 不可达不能影响状态刷新、Diff、Commit、构建、启动、日志、停止和其他本地能力。

#### Context First

优先发送结构化摘要和必要片段，不默认发送整个 Repository。上下文必须说明来源、范围、时间点和是否经过脱敏。

#### Least Privilege

每个 AI 角色有明确的工具白名单、上下文范围和写权限。第一期只开放只读工具。

#### 可追溯

用户可以知道一次请求：使用了哪个 Provider、哪个模型、哪些上下文、哪些被排除、是否进行了脱敏、产生了什么结果，以及是否执行了后续动作。

### 2.3 非目标

本阶段不建设以下能力：

- IDE 级代码补全、AST 重构、全量代码索引或 Java 语义分析；
- 自研 Maven/Gradle 解析器或替代 Maven/Gradle 执行；
- 默认自动修复代码、自动提交、自动 Push 或自动切换分支；
- 任意 shell Agent、无限制插件 Agent 或后台自主循环 Agent；
- 训练模型、托管模型或在应用内维护模型权重；
- 将 API Key 写入项目配置、Git、普通 SQLite、LocalStorage 或日志；
- 以 AI 取代 Runtime 的确定性错误分类、端口检测、依赖解析和进程管理。

## 3. 用户与核心场景

### 3.1 目标用户

| 用户 | 主要诉求 | AI 价值 |
|---|---|---|
| 开发人员 | 快速理解变更、提交和冲突 | Diff Review、Commit Message、Conflict Resolution |
| 联调人员 | 判断服务为什么起不来 | Runtime 失败诊断、日志解释、配置建议 |
| 测试人员 | 了解当前环境和服务状态 | Workspace/Runtime 状态问答、日志摘要 |
| 运维人员 | 快速定位启动和依赖问题 | 多服务故障关联、端口和进程分析 |
| 产品/演示人员 | 不熟悉工程细节但需要运行项目 | 用自然语言查询服务状态和启动结果 |
| 外部 AI Agent | 以工具方式使用 GitWorkspace | 只通过受控 API 调用发现、构建、运行、读日志能力 |

### 3.2 首批高价值场景

#### 场景 A：Runtime 启动失败诊断

用户在 Runtime Dashboard 看到 `BuildFailed`、`ProcessStartFailed`、`PortOccupied` 或 `ProcessCrashed`，点击“AI 分析”。系统生成包含结构化错误、模块、端口、JDK、Maven、构建命令摘要和日志尾部的 Preview。用户确认后，AI 返回：

- 可能原因，按置信度排序；
- 证据来源；
- 排查路径；
- Suggested Actions；
- 不修改项目的人工处理建议。

#### 场景 B：Runtime 日志异常解释

用户在日志页选中一段异常或堆栈，AI 解释异常类型、调用链关键点、常见原因、当前项目上下文和下一步排查建议。只发送用户选中的内容及必要的结构化上下文。

#### 场景 C：多仓库变更总结

用户从 Change Set 或 Workspace Dashboard 选择多个 Repository，生成 Commit Summary、PR Description 或风险摘要。系统显示参与文件和内容 Preview，允许排除 Repository、目录或文件。

#### 场景 D：提交信息生成

根据 staged/worktree diff 生成标题和正文。用户可以编辑结果，最终仍由现有 Commit 流程和 T-08 安全检查完成提交。

#### 场景 E：冲突解决建议

向 AI 发送 Base/Ours/Theirs 和必要项目上下文，返回建议内容和 Diff Preview。用户确认后才进入 T-16 的 Apply/Mark Resolved 流程。

#### 场景 F：自然语言查询应用状态

例如“当前有哪些服务正在运行？”、“gateway 为什么没有启动？”、“最近一次构建失败在哪个模块？”。第一期只调用只读工具，不自动操作。

## 4. 需求规划

### 4.1 功能优先级

| 优先级 | 功能 | 说明 |
|---|---|---|
| P0 | AI 基础设施 | Provider、模型、凭证、Gateway、Secret、Preview、错误和降级 |
| P0 | Runtime 只读助手 | 启动失败、日志异常、结构化错误解释 |
| P1 | AI Git Assistant | Commit Message、Review、Summary、PR Description、Explanation |
| P1 | Conflict Resolution | 建议、Diff Preview、用户确认后 Apply |
| P1 | 应用内统一会话 | 全局入口、上下文切换、历史会话、取消和重试 |
| P2 | 配置建议 | 根据模块数、Spring 版本、JDK 等给出 Runtime 建议 |
| P2 | 受控写操作 | 结构化 Action Proposal，接入现有任务和确认机制 |
| P2 | 外部 Agent 工具接口 | MCP/CLI/API 适配，复用内部 Tool Registry |
| P3 | 插件化 Agent 角色 | 与 T-32 插件系统结合，需单独定义沙箱和权限模型 |

### 4.2 分阶段交付

#### Phase A：AI Foundation

目标是把当前 `ai_review` 从一次性命令升级为统一服务层。

交付内容：

- Provider 配置模型和模型目录；
- API Key 的 OS Credential Store 存取；
- AI Gateway；
- 请求超时、取消、错误分类、重试边界；
- Secret 检测、脱敏、Preview、排除项；
- 会话和 AI 请求历史的最小模型；
- 兼容现有 `ai_review`，但移除模型硬编码和前端直接传 Key；
- AI Settings 页面。

#### Phase B：Runtime Assistant

对应 R-26，优先交付只读排障：

- `BuildFailed`、`ProcessStartFailed`、`PortOccupied`、`ProcessCrashed` 诊断；
- Runtime 日志选段分析；
- 结构化错误与日志上下文联动；
- AI 未配置时隐藏入口或显示可行动的配置提示；
- 结果可复制、可重试、可查看上下文来源。

#### Phase C：Git Assistant

对应 T-25、T-26、T-27：

- Commit Message / Summary；
- Code Review / Security Review / Bug Detection；
- PR Description；
- Commit/File Explanation；
- Conflict Resolution 的建议与预览；
- 所有场景复用 Diff Preview、Secret Scan、排除和用户确认。

#### Phase D：统一应用助手

- 全局 Assistant Drawer 或独立 Assistant View；
- 当前页面、当前 Workspace、当前 Repository、当前 Runtime 自动作为可选上下文；
- 多轮会话、会话重命名、清除和导出；
- 角色切换和上下文范围提示；
- 只读工具调用结果可视化。

#### Phase E：受控 Agent Actions

- 对 Commit、Stage、Runtime Start/Stop、Apply Conflict 等动作生成结构化提案；
- 统一显示影响范围和风险等级；
- 复用现有命令注册表、Task Queue、Command Safety 和 Operation Log；
- 用户确认后提交任务，不允许模型直接调用任意系统命令。

## 5. 总体架构

### 5.1 架构分层

```text
┌─────────────────────────────────────────────────────────────┐
│ UI / External Adapter                                       │
│ Assistant Drawer | Runtime Error | Logs | Git Actions       │
│ MCP / CLI / Future API                                      │
└───────────────────────────────┬─────────────────────────────┘
                                │ typed request
┌───────────────────────────────▼─────────────────────────────┐
│ AI Application Service                                      │
│ ConversationService | TaskService | SuggestionService      │
└───────────┬───────────────────┬─────────────────────────────┘
            │                   │
┌───────────▼──────────┐ ┌──────▼────────────────────────────┐
│ Context Builder       │ │ Policy / Safety                   │
│ Git / Runtime / Logs  │ │ Secret Scan / Redact / Preview    │
│ Diff / Workspace      │ │ Exclude / Confirmation / Audit    │
└───────────┬──────────┘ └──────┬────────────────────────────┘
            │                   │
            └───────────┬───────┘
                        ▼
┌─────────────────────────────────────────────────────────────┐
│ AI Gateway                                                  │
│ Request normalization | token budget | timeout | cancel     │
│ structured output | response parsing | caching | logging    │
└───────────────────────────────┬─────────────────────────────┘
                                │ ProviderAdapter
┌───────────────────────────────▼─────────────────────────────┐
│ Providers                                                    │
│ OpenAI-compatible | Ark | Ollama/local | Enterprise Gateway  │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Tool Registry                                                │
│ Typed read tools now; proposal-backed write tools later     │
│ Reused by UI and external Agent adapters                    │
└─────────────────────────────────────────────────────────────┘
```

### 5.2 推荐代码落点

前端：

```text
src/
├── api/ai.ts                         # IPC API
├── components/ai/
│   ├── AssistantDrawer.vue
│   ├── ConversationView.vue
│   ├── AiRequestPreview.vue
│   ├── AiSuggestionCard.vue
│   └── AiActionProposal.vue
├── composables/useAiAssistant.ts
├── stores/ai.ts
├── types/ai.ts
└── views/AiSettingsView.vue
```

后端：

```text
src-tauri/src/
├── ai/
│   ├── mod.rs
│   ├── gateway.rs                    # 统一调用入口
│   ├── provider.rs                   # Provider trait 和适配器
│   ├── model.rs                      # 模型能力和配置
│   ├── credentials.rs                # OS Credential Store
│   ├── context.rs                    # 上下文构建、摘要和预算
│   ├── session.rs                    # 会话和消息
│   ├── policy.rs                     # 权限、Preview、确认
│   ├── tools.rs                      # 类型化工具注册表
│   ├── cache.rs                      # 结果缓存
│   └── redact.rs                     # 统一脱敏调用适配
└── commands/
    └── ai.rs                         # IPC 薄适配层，逐步兼容旧命令
```

具体模块应继续遵守 Runtime 文档要求的 `src-tauri/src/{runtime,maven,java,process}/` 边界；AI 层只编排，不重复实现 Runtime 或 Git 领域逻辑。

## 6. AI Provider 与模型管理

### 6.1 Provider 模型

Provider 表示一个 API 服务来源，不等同于某个模型。一个 Provider 可以提供多个模型。

推荐字段：

| 字段 | 说明 |
|---|---|
| `id` | 本地稳定 ID，不使用 API Key 作为标识 |
| `name` | UI 展示名称 |
| `kind` | `openaiCompatible` / `ark` / `ollama` / `custom` |
| `baseUrl` | API 基础地址，不包含 Secret |
| `credentialRef` | OS Credential Store 的引用 |
| `enabled` | 是否允许使用 |
| `networkPolicy` | `onlineOnly` / `localOnly` |
| `createdAt` / `updatedAt` | 审计和配置管理 |

### 6.2 模型能力

模型目录不能只保存名称，还应保存能力，用于任务选择和 UI 校验：

| 能力 | 作用 |
|---|---|
| `chat` | 普通多轮对话 |
| `structuredOutput` | 返回可校验 JSON |
| `toolCalling` | 未来调用类型化工具 |
| `vision` | 预留，不作为第一期依赖 |
| `maxContextTokens` | 上下文预算计算 |
| `supportsStreaming` | 是否允许流式响应 |
| `supportsReasoning` | 是否支持推理参数 |

模型配置建议字段：

```json
{
  "id": "provider-model-id",
  "providerId": "openai-default",
  "displayName": "Team Review Model",
  "capabilities": ["chat", "structuredOutput", "toolCalling"],
  "maxContextTokens": 128000,
  "temperature": 0.2,
  "enabled": true
}
```

### 6.3 任务级默认模型

全局默认模型之外，应允许为任务指定模型：

```text
defaultChatModel
defaultRuntimeDiagnosticModel
defaultGitReviewModel
defaultCommitMessageModel
defaultConflictModel
```

选择顺序：

```text
任务显式选择
  > Workspace 任务配置
  > 全局任务默认
  > 全局聊天默认
  > 首个可用模型
```

若模型不具备所需能力，Gateway 在请求前返回可操作错误，不应等 Provider 返回模糊失败。

### 6.4 API Key 与凭证

必须遵守现有全局约束：API Key 不落盘保存。推荐实现：

- Windows：Windows Credential Manager；
- macOS：Keychain；
- Linux：Secret Service；
- 凭证存储不可用时，不自动回退到普通文件；可以允许本次会话临时输入；
- SQLite 只保存 `credentialRef` 和 Provider 元数据；
- 日志、错误、诊断信息和导出数据不得包含 Key；
- 前端不保存明文 Key 到 Pinia 持久化、LocalStorage 或 URL。

## 7. AI Gateway 详细设计

### 7.1 请求模型

Gateway 接收领域无关但类型化的请求：

```text
AiRequest {
  requestId
  sessionId?
  taskKind
  providerId?
  modelId?
  systemInstruction
  messages
  contextManifest
  responseFormat
  toolPolicy
  tokenBudget
  temperature?
  stream
}
```

`contextManifest` 必须列出上下文来源，而不是只发送拼接后的字符串：

```text
ContextItem {
  kind: diff | log | error | repository | runtime | dependency | file
  sourceId
  displayName
  charCount
  estimatedTokens
  redacted
  excluded
}
```

### 7.2 Provider Adapter

Provider Adapter 负责协议差异，不负责业务 prompt：

```text
trait AiProvider {
    fn validate_model(&self, model: &ModelConfig) -> AppResult<CapabilitySet>;
    async fn complete(&self, request: ProviderRequest) -> AppResult<ProviderResponse>;
    async fn stream(&self, request: ProviderRequest) -> AppResult<ProviderStream>;
}
```

第一期可先实现 OpenAI-compatible Adapter；其他 Provider 通过相同接口接入。Adapter 应处理：

- URL 和认证头；
- chat completion 请求格式；
- structured output 参数映射；
- 流式 chunk 解析；
- Provider 错误归一化；
- 请求取消和网络超时。

### 7.3 请求生命周期

```text
Created
  → ContextBuilding
  → SecretScanning
  → PreviewRequired
  → UserApproved
  → Queued
  → Sending
  → Streaming / Parsing
  → Succeeded

任意阶段可进入:
  Cancelled / Rejected / Failed / Degraded
```

发送前 Preview 未确认时，Gateway 不得访问网络。用户修改排除项后必须重新计算 Secret 扫描、token 估算和内容哈希。

### 7.4 失败与重试

可重试：

- 临时网络错误；
- Provider 429；
- Provider 5xx；
- 流式连接中断。

不可自动重试：

- API Key 无效；
- 模型不存在或能力不匹配；
- Secret 检测未通过；
- 用户未确认 Preview；
- 请求内容超出模型上下文限制；
- Provider 返回策略拒绝。

默认最多一次自动重试，并使用退避。重试不得导致重复写操作，因为第一期 Gateway 不直接执行写操作。

## 8. 上下文与 Prompt 设计

### 8.1 上下文来源

上下文构建器只调用现有领域服务，不直接扫描用户项目或重复打开 Git/Runtime 数据：

| 上下文 | 现有来源 |
|---|---|
| Workspace | Workspace/Repository store 和 T-01 Scanner 数据 |
| Git Status | Status Engine、Repository API |
| Diff | T-04/T-12 Diff 能力 |
| History | Graph/History 能力 |
| Conflict | T-16 Conflict Resolver |
| Runtime 配置 | R-07 Runtime Config |
| 依赖和 Closure | R-02/R-03 |
| Build 状态 | R-09/R-12/R-14 |
| 进程和端口 | R-10/R-16 |
| 日志 | R-11/R-13 |
| JDK/Maven | R-04/R-05 |

### 8.2 上下文预算

必须采用尾部优先、结构优先和按任务分块策略：

- 错误诊断：结构化错误 > 最近错误日志 > 日志尾部 > 环境摘要；
- 日志分析：用户选中范围 > 异常堆栈 > 前后少量上下文；
- Code Review：文件清单和 hunk 结构 > 具体 diff；
- Commit Message：变更文件、状态、diff 摘要 > 完整 diff；
- 多仓库 Summary：每仓库摘要 > 所有文件逐行内容。

预算超限时，UI 必须显示被截断或排除的项目。不得静默把整个上下文强行发送。

### 8.3 Prompt 分层

Prompt 分为：

1. **平台系统约束**：AI as Assistant、不得宣称已执行、不得伪造工具结果、不得暴露 Secret；
2. **角色约束**：Runtime Diagnostician、Git Reviewer 等；
3. **任务指令**：本次要生成什么结果；
4. **结构化上下文**：带来源标签的事实；
5. **输出 Schema**：要求返回结构化结果。

业务代码不得通过字符串拼接把用户内容插入系统约束。用户日志、diff、文件内容都应作为不可信数据，并在 Prompt 中显式标记为参考内容。

### 8.4 结果模型

建议统一为以下结果类别：

```text
AiResult
├── Answer              # 普通解释
├── DiagnosticReport    # 原因、证据、排查路径、建议
├── ReviewReport        # summary、issues、file、line、severity
├── GeneratedText       # commit message、PR description
├── ConflictProposal    # proposed content + diff
└── ActionProposal      # 未来的结构化待确认动作
```

结果必须区分：

- AI 推断；
- GitWorkspace 确定性事实；
- 用户需要确认的建议；
- 已执行的实际动作。

## 9. 内置智能体设计

### 9.1 产品形态

对用户提供一个统一的 **GitWorkspace Assistant** 入口，可以是右侧 Drawer 或独立 Assistant 页面。入口支持从以下位置带入上下文：

- Workspace Dashboard；
- Repository/Changes/Diff；
- Conflict Resolver；
- Runtime Dashboard；
- Runtime Error Alert；
- Runtime Logs。

入口不应让用户必须先理解 Agent、Tool、Prompt 等技术概念。界面上应展示当前作用域，例如“当前工作区 / 3 个仓库 / Runtime gateway / 选中日志 86 行”。

### 9.2 角色模型

统一助手内部包含受限角色：

| 角色 | 允许读取 | 输出 | 第一阶段写权限 |
|---|---|---|---|
| Workspace Assistant | Workspace、Repository、Status、History | 状态解释、摘要、导航建议 | 无 |
| Git Reviewer | Diff、History、文件元数据 | Review、风险和安全建议 | 无 |
| Commit Assistant | Diff、Change Set、历史提交 | Commit Message/Summary | 无 |
| Conflict Assistant | Base/Ours/Theirs、冲突元数据 | 解决建议和 Diff | 无 |
| Runtime Diagnostician | 错误、日志、进程、端口、JDK、Maven、Closure | 诊断和排查建议 | 无 |
| Runtime Config Advisor | Runtime 配置和项目摘要 | VM/Profile/启动配置建议 | 无 |
| Action Planner | 以上只读上下文 | 结构化 Action Proposal | 无，后续需确认 |

角色选择可以由入口自动推断，也允许用户手动选择。自动推断结果必须在 UI 中可见。

### 9.3 工具注册表

工具是应用能力的类型化包装，不是任意函数执行器。第一期工具全部只读：

```text
workspace.list
repository.list
repository.status
repository.diff
repository.history
repository.conflicts
runtime.listApplications
runtime.getConfig
runtime.getProcessStatus
runtime.getClosure
runtime.getLogs
runtime.getErrorContext
jdk.list
maven.detect
task.getStatus
```

每个工具定义：

- 稳定名称和版本；
- JSON Schema 输入；
- 允许的角色；
- 允许的上下文范围；
- 是否需要当前 Workspace；
- 是否可能包含 Secret；
- 超时和结果大小上限；
- 审计字段。

未来写工具不得直接修改领域状态，只能返回 Action Proposal。例如：

```text
git.createCommitProposal
runtime.startProposal
conflict.applyProposal
runtime.updateConfigProposal
```

真正执行仍调用已有命令和任务系统。

### 9.4 Agent 循环边界

第一期不做无限自主循环。一次用户请求最多允许有限次工具调用，建议默认上限为 8 次，达到上限后返回“需要用户继续确认/缩小范围”。

Agent 不得：

- 自己决定扩大 Workspace、Repository 或文件范围；
- 在后台持续观察并自动触发请求；
- 因工具失败自行改用 shell 命令；
- 伪造未调用工具的事实；
- 将一条自然语言指令拆成多个未展示的危险操作。

## 10. 安全、隐私和用户确认

### 10.1 发送 Preview

Preview 是硬要求，不是调试选项。发送前页面必须展示：

- Provider 和模型；
- 请求类型；
- 目标 Workspace/Repository/Runtime；
- 文件、目录、日志片段和结构化字段清单；
- 每项字符数和估算 token 数；
- Secret 检测结果；
- 自动脱敏项；
- 被排除项；
- 预计请求次数和可用时的成本估算；
- 是否会使用网络；
- 明确的“确认发送”按钮。

### 10.2 Secret 处理策略

复用 T-08 的 Secret Protection，不另起规则：

1. `Block`：发现私钥、API Key、密码、Token 等高风险内容时默认阻止；
2. `Mask`：规则明确且可以安全替换时显示替换后的 Preview；
3. `Exclude`：用户排除文件、目录或日志项后重新构建请求；
4. `Warn`：低置信度发现只允许在用户明确确认后发送。

Secret 检测必须发生在最终内容生成之后，而不是只扫描原始文件。脱敏后的内容仍需再次检查。

### 10.3 写操作确认

未来 Action Proposal 统一包含：

```text
ActionProposal {
  proposalId
  actionKind
  riskLevel
  targetScope
  affectedRepositories
  affectedFiles
  beforeSummary
  afterSummary
  diff?
  commandPreview?
  reversible
  expiresAt
}
```

用户确认前：

- 不修改用户项目；
- 不改变 Git 状态；
- 不写 Runtime 配置；
- 不执行进程或脚本。

确认后必须进入现有命令、Task Queue、Command Safety 和 Operation Log。AI 层不直接持有 `Repository` 句柄，也不直接 spawn 子进程。

### 10.4 数据保留

默认不保存完整 Prompt 中的敏感原文。建议保存：

- 请求类型、Provider、模型；
- 上下文 manifest；
- 内容 hash；
- Secret 检测结果数量和类别，不保存 Secret 原文；
- 请求状态、耗时、token 使用量（Provider 返回时）；
- 结构化结果；
- 用户确认和后续动作记录。

完整会话是否保存由用户设置决定。删除会话应同时删除消息内容和相关本地缓存。

## 11. 数据模型与存储

### 11.1 存储分层

| 数据 | 存储 | 说明 |
|---|---|---|
| Provider 元数据 | SQLite | 不含 API Key |
| Model 元数据 | SQLite | 可迁移、可校验 |
| API Key | OS Credential Store | 只存引用 |
| 会话元数据 | SQLite | 标题、角色、作用域、时间 |
| 会话消息 | SQLite 或可选本地文件 | 按设置控制是否持久化 |
| 请求审计 | SQLite | 状态、hash、来源、token、错误 |
| AI 结果缓存 | SQLite + 内存 LRU | 按输入 hash 和模型隔离 |
| 临时 Preview | 内存 | 应用退出即清除 |
| Runtime/Git 上下文 | 现有模块 | AI 不复制一份领域数据 |

### 11.2 建议表

当前已有 `ai_reviews` 和 `ai_tasks`。后续应通过版本化迁移增加或逐步替换为：

```text
ai_providers
- id, name, kind, base_url, credential_ref, enabled, created_at, updated_at

ai_models
- id, provider_id, display_name, capabilities_json,
  max_context_tokens, defaults_json, enabled, created_at, updated_at

ai_task_defaults
- task_kind, model_id, workspace_id nullable, updated_at

ai_sessions
- id, title, role, workspace_id nullable, repository_scope_json,
  runtime_scope_json, created_at, updated_at, archived_at nullable

ai_messages
- id, session_id, role, content_json, sequence, created_at

ai_requests
- id, session_id nullable, task_kind, provider_id, model_id,
  input_hash, context_manifest_json, status, error_code nullable,
  input_tokens nullable, output_tokens nullable, latency_ms nullable,
  created_at, finished_at

ai_proposals
- id, request_id, action_kind, risk_level, target_scope_json,
  diff_json nullable, status, confirmed_at nullable, executed_task_id nullable
```

迁移必须遵守现有 SQLite WAL、单写者、版本化迁移和向后兼容约束。`ai_reviews`/`ai_tasks` 的历史数据应保留或提供兼容读取，不做破坏性删除。

### 11.3 缓存策略

缓存 Key 至少包含：

```text
taskKind + modelId + promptVersion + contextHash + settingsHash
```

以下变化必须失效缓存：

- diff 或日志内容变化；
- Runtime 错误上下文变化；
- 模型或 Provider 变化；
- Prompt 版本变化；
- Secret 脱敏/排除策略变化。

缓存不得跨不同模型或不同 Provider 复用。缓存结果必须标记生成时间和上下文 hash，UI 不能把过期结果显示成当前事实。

## 12. IPC 与前端设计

### 12.1 IPC 原则

Rust serde 类型是 IPC 的单一事实来源，继续使用 golden-file 快照测试。IPC 层只负责校验和调用服务，不把业务逻辑堆在 command 函数中。

建议命令：

```text
ai_list_providers
ai_save_provider
ai_remove_provider
ai_test_provider
ai_list_models
ai_save_model
ai_set_task_default_model
ai_get_settings_summary

ai_create_session
ai_list_sessions
ai_get_session
ai_delete_session
ai_send_preview
ai_approve_request
ai_cancel_request
ai_retry_request
ai_get_request_status
ai_get_request_audit

ai_runtime_diagnose
ai_runtime_explain_log
ai_git_review
ai_generate_commit_message
ai_generate_pr_description
ai_propose_conflict_resolution
```

`ai_approve_request` 只批准发送请求，不等于批准后续写操作。Action Proposal 必须有独立确认命令。

### 12.2 设置页面

在现有“设置”导航下新增 `AI 设置`，分为：

1. **Provider**：新增、编辑、启用/禁用、测试连接；
2. **模型**：模型 ID、显示名、能力、上下文长度、默认参数；
3. **任务默认值**：Runtime 诊断、日志解释、Review、Commit、Conflict 等；
4. **隐私与安全**：Preview 开关（不可关闭的硬要求）、Secret 策略、会话保存、日志保留；
5. **用量与诊断**：请求次数、token（可用时）、最近错误、清除缓存；
6. **凭证**：设置、替换、删除 Key，不展示完整 Key。

页面不能把 API Key 写入普通表单状态的持久化机制。测试连接只返回成功、失败原因和模型能力，不返回响应中的敏感内容。

### 12.3 Assistant Drawer

建议使用全局右侧 Drawer：

- 顶部：当前角色、模型、上下文范围；
- 中部：会话消息、工具读取摘要、建议卡片；
- 底部：输入框、发送、取消、清空上下文；
- 发送前：Preview Modal；
- 建议结果：复制、重新生成、查看来源、生成 Action Proposal；
- 失败状态：配置 AI、重试、缩小范围、转到日志/Runtime 页面。

不在每个视图内重复实现一套聊天状态。领域页面只负责提供上下文入口和专用快捷动作。

### 12.4 未配置和离线状态

AI 未配置时：

- 核心页面继续正常工作；
- AI 入口可以隐藏，或显示“配置 AI”引导；
- 错误提示提供打开 AI Settings 的动作；
- 不弹出要求用户临时输入 Key 的浏览器原生 prompt；
- 不因为 AI 失败阻塞构建、启动、停止、Commit 或日志查看。

AI 网络不可用时：

- 显示离线/网络错误；
- 保留用户的 Preview 和输入上下文，允许重试；
- 不能假装使用了本地模型；
- 只读确定性信息继续可用。

## 13. Runtime Assistant 详细方案

### 13.1 失败诊断输入

对应 R-14 的结构化错误，优先发送：

```text
error.code
error.message
error.details: module / pid / port / processName / runtime / reason
runtime config summary
jdk and maven summary
build command preview
log tail or selected exception
```

不发送：

- 未选择的完整日志；
- 敏感环境变量的值；
- API Key、密码、私钥；
- 与诊断无关的整个项目源码。

### 13.2 诊断结果

```text
DiagnosticReport {
  headline
  confidence
  facts[]
  likelyCauses[]
  suggestedActions[]
  needsUserCheck[]
  sourceContext[]
}
```

`facts` 只能来自 GitWorkspace 传入的确定性上下文；`likelyCauses` 和 `suggestedActions` 必须标记为 AI 建议。AI 不得输出“已重启”“已修复”等未执行事实。

### 13.3 入口接入

- `RuntimeErrorAlert`：对 Build/Start/Port/Crash 错误增加“AI 分析”入口，未配置时转到 Settings；
- `RuntimeLogsView`：支持选中日志片段后分析；
- `RuntimeDashboard`：支持对当前应用和最近一次失败请求诊断；
- 诊断请求和结果与具体 `processId`、`runtimeName`、错误发生时间关联。

### 13.4 与 Runtime 边界

Runtime Assistant 不能：

- 修改 `runtimes/*.json`；
- 修改 Maven 项目或源码；
- 直接运行 Maven、Java、脚本；
- 绕过 R-14 的错误分类和 Command Safety；
- 阻塞 Runtime 主链路。

## 14. Git Assistant 详细方案

### 14.1 公共 Diff 上下文管道

T-25/T-26/T-27 应共享：

- 文件级选择和排除；
- staged/worktree/base/ours/theirs 来源标记；
- diff 结构摘要；
- 行数和 token 预算；
- Secret Scan 和 Mask/Exclude；
- 输入 hash 和结果缓存；
- Preview 和用户确认；
- 结构化结果解析。

差异只保留在任务 prompt 和结果 Schema，避免三个任务重复实现 HTTP 和安全链路。

### 14.2 Commit Message

输入：选定范围的 diff、文件状态、最近提交风格（可选）。

输出：

```text
CommitSuggestion {
  title
  body[]
  type?
  scope?
  changedRepositories[]
  rationale
}
```

结果可编辑，但 Commit 仍进入现有 T-11 和 T-08 安全检查。AI 不直接执行 Commit。

### 14.3 Conflict Resolution

输入 Base/Ours/Theirs、冲突文件路径、必要上下文；输出建议文件内容和前后 Diff。应用流程必须是：

```text
AI Suggestion
  → Diff Preview
  → User Confirmation
  → T-16 Apply
  → Mark Resolved
```

大文件按 hunk 分批，不发送整个 Repository。

## 15. 外部 Agent 能力规划

产品文档中的 AI Agent 不一定要等同于应用内聊天助手。建议把内部 Tool Registry 作为未来 MCP/CLI/API 的唯一工具来源：

```text
Internal Tool Registry
        ├── UI Assistant
        ├── MCP Adapter
        ├── CLI Adapter
        └── Future HTTP/API Adapter
```

外部 Agent 第一阶段只提供：

- Workspace/Repository 发现；
- 状态和 Diff 查询；
- Runtime 配置、Closure、进程和日志查询；
- Build/Run 的 Action Proposal。

执行类能力必须要求外部调用方携带确认标记，并由 GitWorkspace 重新执行权限、范围和安全校验。不能因为来自 MCP/CLI 就绕过 UI 的确认规则。

与 T-32 插件系统的关系：AI Tool 不应直接复用任意脚本插件。插件权限、沙箱、来源信任和审计模型成熟后，再考虑允许用户注册 AI 工具。

## 16. 性能、跨平台与可观测性

### 16.1 性能

- AI 请求必须异步化，不阻塞 IPC、Runtime 状态机和日志采集；
- 大日志和大 diff 必须摘要、截断或分块；
- 前端流式输出合帧，避免每个 token 触发完整页面重渲染；
- 工具结果设置数量和 payload 上限；
- 会话列表分页，消息内容按需加载；
- 结果缓存使用有上限的内存 LRU，持久缓存按 hash 管理；
- 同一上下文的重复请求可复用缓存，但必须校验模型、Provider 和 Prompt 版本；
- AI 请求不参与 Maven/Java 子进程并发预算，但仍应有独立的请求并发上限；
- 不得因为打开 AI 设置或 Assistant 页面触发全量 Repository 扫描。

### 16.2 跨平台

- Provider URL 处理使用结构化 URL，不手写字符串拼接；
- OS Credential Store 按 Windows/macOS/Linux 分支实现；
- 网络和 TLS 错误转换为统一错误，不依赖 shell 命令；
- 日志路径、缓存路径和凭证引用使用平台路径 API；
- API Key 不能出现在进程命令行，避免 Windows/macOS/Linux 的进程列表泄漏；
- 代理、证书和企业网络差异应作为 Provider 配置或系统网络能力处理。

### 16.3 日志与审计

复用 T-08 的 `ai.log`，记录：

- requestId、taskKind、Provider/model ID；
- 生命周期状态、耗时、重试次数；
- 上下文 item 数量、token 估算和脱敏计数；
- 错误 code 和 recoverable；
- 不记录 API Key、完整 Prompt、Secret 原文和未经用户同意的完整代码内容。

用户可在 AI 设置中查看请求统计和清理本地历史，但不应通过日志导出泄露请求内容。

## 17. 错误模型

建议在现有 `AppError::Ai` 基础上增加可区分的结构化错误 code：

```text
AiNotConfigured
AiCredentialUnavailable
AiProviderUnavailable
AiAuthenticationFailed
AiModelNotFound
AiModelCapabilityMismatch
AiSecretDetected
AiPreviewRequired
AiRequestCancelled
AiRateLimited
AiContextTooLarge
AiResponseInvalid
AiPolicyRejected
AiActionConfirmationRequired
```

每个错误都应携带：

- `message`：用户可读原因；
- `details`：Provider、模型、请求阶段、上下文数量等非敏感字段；
- `recoverable`：是否可重试；
- `suggestedActions`：配置、缩小范围、排除文件、重新发送、查看日志等。

## 18. 测试与验收

### 18.1 单元测试

- Provider 配置序列化和默认值；
- 模型能力校验；
- 请求生命周期状态迁移；
- token 估算、摘要和截断；
- Prompt version 和输入 hash；
- Secret 检测、Mask、Exclude 和二次扫描；
- Provider 错误映射；
- structured output 解析和非法响应降级；
- 工具权限矩阵；
- Action Proposal 不会直接执行。

### 18.2 集成测试

- 使用 fake OpenAI-compatible Provider 测试成功、流式、超时、取消、429、5xx 和非法 JSON；
- Runtime 端口占用、依赖缺失、JDK/Maven 不可用和进程崩溃均能生成正确上下文；
- AI 未配置时 Runtime/Git 核心操作仍可用；
- AI 请求前发现 AWS Key、JWT、私钥、密码和 Token 时阻断；
- 排除敏感文件后 Preview 内容不再包含被排除内容；
- 结果缓存只在相同模型、Prompt 和 Context hash 下命中；
- Conflict 建议未确认时不修改工作区；
- Commit 建议未确认时不提交；
- IPC golden 快照与 TypeScript 类型一致；
- 会话删除不会残留完整 Prompt 或 API Key。

### 18.3 前端验收

- Provider、模型和默认任务配置可管理；
- API Key 输入后不回显、不进入 LocalStorage；
- Preview 能列出发送范围、脱敏和排除项；
- 请求可取消、重试和查看失败原因；
- Assistant Drawer 在不同 Workspace/Runtime 范围间不会串上下文；
- AI 未配置/离线状态有明确降级；
- 长响应流式渲染不阻塞现有页面；
- Action Proposal 显示影响范围、风险和 Diff。

### 18.4 安全验收

- 代码走查确认 AI 无直接 shell 执行路径；
- 代码走查确认 AI 无直接修改用户项目和 Git 状态路径；
- 凭证只通过 OS Credential Store 访问；
- `ai.log`、错误、审计和导出内容无明文 Secret；
- 外部 Agent 不能绕过工具权限和确认机制；
- 用户取消、关闭窗口或网络断开时无残留未确认动作。

## 19. 实施任务拆分

建议新增一个跨模块的设计任务作为 T/R AI 任务的前置，而不是直接在 R-26 中扩展全部基础设施：

| 编号建议 | 任务 | 依赖 |
|---|---|---|
| AI-01 | Provider、Model、Credential 与 AI Settings | T-08 |
| AI-02 | AI Gateway、请求状态、错误、取消和 Provider Adapter | AI-01、T-08 |
| AI-03 | Context Builder、Preview、Secret、Exclude、token 预算 | AI-02、T-04、R-11 |
| AI-04 | Session、Message、Request Audit、缓存 | AI-02、T-03 |
| AI-05 | Tool Registry 与只读 Workspace/Runtime 工具 | AI-02、R-12/R-13 |
| AI-06 | Runtime Assistant | AI-03、AI-05、R-11、R-14 |
| AI-07 | Git Assistant 公共 Diff 管道 | AI-03、T-04、T-08 |
| AI-08 | Commit/Review/PR/Explanation | AI-07、T-25/T-27 |
| AI-09 | Conflict Resolution | AI-07、T-16 |
| AI-10 | 统一 Assistant Drawer | AI-04、AI-05、AI-06 |
| AI-11 | Action Proposal 与确认执行 | AI-05、T-05、T-24、T-34 |
| AI-12 | MCP/CLI 外部 Agent Adapter | AI-05、T-31/T-32 |

这些编号是实施建议，不应在未确认排期前直接替换现有 T/R 任务编号。现有 T-25/T-26/T-27 和 R-26 应在各自 spec 中补充“依赖 AI Foundation”的关系。

## 20. 主要风险与应对

| 风险 | 影响 | 应对 |
|---|---|---|
| Secret 被发送到第三方模型 | 高 | Preview、统一扫描、Mask/Exclude、默认阻断、二次扫描 |
| AI 输出不可靠 | 中 | 事实与推断分离、显示来源、只做建议、用户确认 |
| Agent 权限过大 | 高 | 工具白名单、角色权限、Proposal、现有任务队列执行 |
| Provider 协议不一致 | 中 | Adapter 层、能力声明、fake Provider 测试 |
| 会话/Prompt 泄露项目代码 | 高 | 默认最小上下文、可选持久化、删除能力、日志不记原文 |
| AI 请求阻塞 Runtime | 高 | 异步任务、独立并发上限、无主链路依赖、可取消 |
| 多任务重复建设 | 中 | AI Foundation 前置任务、统一 Gateway/Context/Policy |
| 模型成本不可控 | 中 | token 预算、Preview、缓存、批量请求成本估算 |
| 外部 Agent 绕过安全机制 | 高 | MCP/CLI 复用内部 Tool Registry，所有写操作重新确认 |
| 本地模型能力不足 | 低 | 模型能力声明，任务级 fallback 和明确错误 |

## 21. 最终决策

1. **需要内置 AI Assistant**，因为 GitWorkspace 同时拥有 Git、Runtime、任务和日志等结构化上下文，单个功能按钮无法覆盖跨模块问题。
2. **不建设无边界万能 Agent**。应用内 Agent 必须是受限角色，第一期只读。
3. **先建设 AI Foundation，再建设 Runtime/Git 场景**。Provider、模型、凭证、Gateway、上下文、Secret 和 Preview 是共同依赖。
4. **Runtime Assistant 作为第一个完整场景**，因为它能直接利用 R-11/R-14 的日志和结构化错误，并且不会和代码写入发生冲突。
5. **AI 模型管理必须进入 Settings/AI**，但模型管理不等于模型下载。第一期管理 Provider、模型 ID、能力和默认任务，不负责托管模型权重。
6. **所有写操作都通过 Action Proposal 接入现有命令和任务系统**，由用户确认后执行。
7. **外部 AI Agent 与应用内 Assistant 共用 Tool Registry**，避免维护两套能力和安全边界。

