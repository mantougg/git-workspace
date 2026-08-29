# GitWorkspace AI Assistant 任务拆解总览

> 来源：`docs/ai-assistant-design.md`（设计草案，下称「设计文档」，各任务表格中的 `§` 号指其章节号）。
> 拆分原则：**按架构分层拆分**（设计文档 §19），每个任务一个独立文档（同目录下 `AI-XX-<slug>.md`），可独立跟踪进度与验收。
> 本文件是唯一的总进度索引；每个任务文档内另有自己的「进度」章节。
>
> 编号用 **AI-XX**，与 Git Workspace 任务（`docs/tasks/` 的 T-XX）、Runtime 任务（`docs/tasks-runtime/` 的 R-XX）区分。
> AI 是应用智能层，**基础设施复用而非重建**：SQLite 数据层（T-03）、Diff（T-04）、Task Queue（T-05）、错误/日志/Secret（T-08）、Runtime 日志与结构化错误（R-11/R-14）、IPC/Task 集成（R-12）。
>
> 横切约束：本目录 [00-全局开发约束.md](./00-全局开发约束.md) 为所有 AI 任务**必读**；涉及 Git/Runtime 联动时，`../tasks/00-全局开发约束.md` 与 `../tasks-runtime/00-全局开发约束.md` 一并生效（各任务文档顶部标注了最小加载集）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**12**
- 已完成：**0** · 进行中：**0** · 未开始：**12**
- 完成度：**0 / 12（0%）**

## 第一阶段口径（设计文档 §21）

- 第一阶段 = **Phase A + Phase B**（AI-01 ~ AI-06）：Provider/凭证/Gateway/上下文/Preview/会话审计/只读工具 + Runtime 只读排障。
- 第一期**只开放只读工具**：不做自动写操作、不做无限自主 Agent 循环（§2.2 / §9.4），写操作一律留待 Phase E 的 Action Proposal。
- Runtime Assistant 是第一个完整场景（§21 决策 4）；Git Assistant 系列（AI-07 ~ AI-09）在其后启动。
- 现有 `ai_review` 原型（`src-tauri/src/commands/ai.rs`）在 Phase A 中兼容保留，但移除模型硬编码与前端直接传 Key（§4.2 Phase A）。

---

## 阶段与任务索引

### Phase A · AI Foundation（前置，P0，4 个）

> 对应设计文档 §4.2 Phase A：把 `ai_review` 从一次性命令升级为统一服务层。Foundation 不稳，上层场景返工成本高。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| AI-01 | Provider / Model / Credential 与 AI Settings | P0 | ⬜ | T-08 | [AI-01-provider-model-credentials.md](./AI-01-provider-model-credentials.md) |
| AI-02 | AI Gateway（请求生命周期 / Provider Adapter / 流式） | P0 | ⬜ | AI-01, T-08 | [AI-02-ai-gateway.md](./AI-02-ai-gateway.md) |
| AI-03 | Context Builder / Preview / Secret / Token 预算 | P0 | ⬜ | AI-02, T-04, R-11 | [AI-03-context-builder-preview.md](./AI-03-context-builder-preview.md) |
| AI-04 | Session / Message / Request Audit / 结果缓存 | P0 | ⬜ | AI-02, T-03 | [AI-04-session-audit-cache.md](./AI-04-session-audit-cache.md) |

### Phase B · 工具注册表与 Runtime Assistant（P0，2 个）

> 对应设计文档 §4.2 Phase B 与 §13。AI-06 即 R-26 的实现载体。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| AI-05 | Tool Registry 与只读 Workspace/Runtime 工具 | P0 | ⬜ | AI-02, R-12, R-13 | [AI-05-tool-registry.md](./AI-05-tool-registry.md) |
| AI-06 | Runtime Assistant（失败诊断 / 日志异常解释） | P0 | ⬜ | AI-03, AI-05, R-11, R-14 | [AI-06-runtime-assistant.md](./AI-06-runtime-assistant.md) |

### Phase C · Git Assistant（P1，3 个）

> 对应设计文档 §4.2 Phase C 与 §14。AI-08 承载 T-25/T-27 场景，AI-09 承载 T-26 场景。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| AI-07 | Git Assistant 公共 Diff 管道 | P1 | ⬜ | AI-03, T-04, T-08 | [AI-07-git-diff-pipeline.md](./AI-07-git-diff-pipeline.md) |
| AI-08 | Commit / Review / PR / Explanation 场景 | P1 | ⬜ | AI-07（覆盖 T-25/T-27 场景） | [AI-08-git-assistant-scenarios.md](./AI-08-git-assistant-scenarios.md) |
| AI-09 | AI Conflict Resolution | P1 | ⬜ | AI-07, T-16（覆盖 T-26 场景） | [AI-09-conflict-resolution.md](./AI-09-conflict-resolution.md) |

### Phase D · 统一应用助手（P1，1 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| AI-10 | 统一 Assistant Drawer 与会话 UI | P1 | ⬜ | AI-04, AI-05, AI-06 | [AI-10-assistant-drawer.md](./AI-10-assistant-drawer.md) |

### Phase E · 受控写与外部 Agent（P2，2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| AI-11 | Action Proposal 与确认执行 | P2 | ⬜ | AI-05, T-05, T-24, T-34 | [AI-11-action-proposals.md](./AI-11-action-proposals.md) |
| AI-12 | 外部 Agent Adapter（MCP / CLI） | P2 | ⬜ | AI-05, T-31, T-32 | [AI-12-external-agent-adapter.md](./AI-12-external-agent-adapter.md) |

---

## 关键依赖链

```text
T-08 Secret ──► AI-01 Provider/凭证/Settings ──► AI-02 Gateway ──┬──► AI-03 Context/Preview ──► AI-06 Runtime Assistant
                                                                 │                                    ▲
T-03 SQLite ────────────────────────────────────────────────────┼──► AI-04 Session/审计/缓存          │
                                                                 └──► AI-05 Tool Registry ────────────┘
AI-03 ──► AI-07 Git Diff 管道 ──► AI-08 Commit/Review/PR/Explanation（承载 T-25/T-27）
                              └──► AI-09 Conflict Resolution（+T-16，承载 T-26）
AI-04 + AI-05 + AI-06 ──► AI-10 统一 Assistant Drawer
AI-05 ──► AI-11 Action Proposal（+T-05/T-24/T-34）
AI-05 ──► AI-12 外部 Agent Adapter（+T-31/T-32）
```

- **Phase A 是全部后续任务的前置**（§21 决策 3）；AI-06 验证通过后再启动 Git Assistant 系列。
- 现有 T-25 / T-26 / T-27 / R-26 保留为产品场景占位 spec（不改编号、不删文档），其实现由本目录任务承载：
  - R-26 ↔ AI-06；T-25 / T-27 ↔ AI-08（经 AI-07）；T-26 ↔ AI-09（经 AI-07）。
  - 对应 AI 任务完成时，在那份 T/R spec 的时间线追加一行「由 AI-XX 实现」并更新其状态。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录。
3. 新增/调整任务时，重新编号并同步依赖字段。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
5. 全局横切约束统一记录在 `00-全局开发约束.md`；各任务文档的「架构/性能注意点」只写该任务特有内容，与全局约束叠加，不重复。
6. 设计文档 `docs/ai-assistant-design.md` 是产品与安全技术约束的单一事实来源；任务 spec 与之冲突时，先改设计文档或在 spec 中显式说明原因与边界。
