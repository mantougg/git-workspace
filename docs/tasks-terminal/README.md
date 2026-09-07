# GitWorkspace 内嵌终端任务总览

> 来源：[../terminal-feature-plan.md](../terminal-feature-plan.md)（方案 C：PTY 交互 Shell + 应用输出镜像，含架构设计与 IPC 契约）。
> 拆分原则：按方案 §6 分期拆为 **TM-XX 任务**，每个任务一个独立文档（同目录下 `TM-XX-<slug>.md`），可独立跟踪进度与验收。
> **设计 spec 与执行状态分离**：架构/IPC 契约/交互基准以 `../terminal-feature-plan.md` 为准（尤其 §4 IPC 契约，前后端联调以它为准）；执行状态以本索引 + 各任务文档「进度」为准。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**7**
- 已完成：**3** · 进行中：**1** · 未开始：**3**
- 完成度：**3 / 7（43%）**

---

## 阶段与任务索引

### 一期 · 终端基础（PTY + 面板 + 交互 Shell，3 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| TM-01 | PTY 会话后端（portable-pty + IPC 契约） | P0 | ✅ | — | [TM-01-pty-backend.md](./TM-01-pty-backend.md) |
| TM-02 | 终端面板前端（xterm 封装 + drawer + tab） | P0 | ✅ | TM-01（契约） | [TM-02-terminal-panel.md](./TM-02-terminal-panel.md) |
| TM-03 | 交互式 Shell 端到端（联通 + 三平台冒烟） | P0 | 🟦 | TM-01, TM-02 | [TM-03-interactive-shell-e2e.md](./TM-03-interactive-shell-e2e.md) |

### 二期 · Git 输出镜像（1 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| TM-04 | Git 输出流式化 + Git Console 镜像 | P1 | ⬜ | TM-03 | [TM-04-git-console-mirror.md](./TM-04-git-console-mirror.md) |

### 三期 · Runtime 终端化（2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| TM-05 | Runtime 输出 xterm tab + App 级订阅 + 面板操作工具条 | P1 | ⬜ | TM-03 | [TM-05-runtime-output-xterm.md](./TM-05-runtime-output-xterm.md) |
| TM-06 | 在终端中启动（LaunchPlan.preview 入 PTY，降级模式） | P2 | ⬜ | TM-05 | [TM-06-launch-in-terminal.md](./TM-06-launch-in-terminal.md) |

### 增量 · 打磨（1 个）

> 非方案分期任务，来自用户需求（来源与决策见任务文档）。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| TM-07 | 终端交互打磨（搜索 / 链接 / 复制粘贴 / profile / 清屏重开 / 最大化） | P2 | ✅ | TM-03 | [TM-07-terminal-polish.md](./TM-07-terminal-polish.md) |

---

## 依赖链

```
TM-01 PTY 后端 ──► TM-02 面板前端 ──► TM-03 端到端联通 ──► TM-04 Git Console 镜像
                                              ──► TM-05 Runtime xterm tab ──► TM-06 在终端中启动
                                              ──► TM-07 交互打磨（增量）
```

- **一期按 TM-01 → TM-03 顺序执行**：TM-02 可在 §4.2 IPC 契约确定后与 TM-01 并行开发（前端先用 mock 数据渲染），但联调验收在 TM-03。
- TM-04 与 TM-05 可在 TM-03 完成后并行。
- TM-06 依赖 TM-05（复用 runtime tab 与订阅上移成果）。
- TM-07 为增量打磨任务（用户需求 2026-09-08），依赖 TM-03，无下游互锁，可与 TM-04/TM-05 并行。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 结果 + 验证命令）。
3. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
4. 新增/调整任务时，重新编号并同步依赖字段与本表。
5. **spec 优先**：架构、IPC 契约（命令/事件名/payload）、交互基准以 `../terminal-feature-plan.md` 为准；开发中发现 spec 需要调整时，先改 spec 再改代码，并在任务时间线注明。
6. 平台兼容性遵守根目录 `AGENTS.md`「平台兼容性开发规范」+ 本目录 `00-全局开发约束.md`（终端特有硬规则）。
