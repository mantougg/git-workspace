# GitWorkspace Node 前端工程启动任务总览

> 来源：`docs/node-frontend-runtime-design.md`（设计稿，下称「设计文档」，各任务表格中的 `§` 号指其章节号）。
> 拆分原则：**按能力层拆分**（工具链 → 发现 → 配置 → 启动 → 检测 → UI → 验收），每个任务一个独立文档（同目录下 `N-XX-<slug>.md`），可独立跟踪进度与验收。
> 本文件是唯一的总进度索引；每个任务文档内另有自己的「进度」章节。
>
> 编号用 **N-XX**（Node 前端工程），与 Git 任务（T-XX）、Runtime 任务（R-XX）、Backend 重构（B-XX）、Desktop 任务（D-XX）、Fix（F-XX）、AI 任务（AI-XX）区分。
> 本套任务是**功能扩展**：在既有 Runtime 引擎上新增第二条技术栈（Node.js 前端工程），**不改写** Maven/Spring Boot 既有链路的语义（设计文档 §2.1 清单为零改动复用区）。
>
> 横切约束：本目录 [00-全局开发约束.md](./00-全局开发约束.md) 为所有 N-XX 任务**必读**；`../tasks-runtime/00-全局开发约束.md`（Runtime 引擎全局约束）与根 `AGENTS.md` 平台兼容性规范一并生效（各任务文档顶部标注了最小加载集）。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**9**
- 已完成：**7** · 进行中：**0** · 未开始：**2**（N-09 条件触发）
- 完成度：**7 / 9（78%）**

## MVP 口径（设计文档 §7）

- MVP = **Phase 0 ~ Phase 2 全部**（N-01 ~ N-07）：工具链检测 → `package.json` 发现 → 配置扩展 → LaunchPlan/引擎 → 检测器 → UI → 端到端验收。
- MVP 只保 **`npm run <script>`**；pnpm/yarn 仅决策链识别、不可用时给可行动错误，真正执行在 N-08。
- MVP 暂不实现：自动 `npm install`（仅 N-08 显式动作）、monorepo workspaces 路由、bun、watch 联动、模板（N-09）。

---

## 阶段与任务索引

### Phase 0 · 工具链与发现（前置，P0，2 个）

> 对应设计文档 §4.1 / §4.2。两个任务相互独立，可并行推进；是全部后续任务的前置。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| N-01 | Node 工具链检测（node / 包管理器决策链 / 版本探测） | P0 | ✅ | — | [N-01-node-toolchain-detection.md](./N-01-node-toolchain-detection.md) |
| N-02 | package.json 发现与索引（SCHEMA_V17 `node_projects`） | P0 | ✅ | —（复用 T-01, T-03） | [N-02-package-json-discovery.md](./N-02-package-json-discovery.md) |

### Phase 1 · 配置与启动闭环（P0，3 个）

> 对应设计文档 §4.3 ~ §4.6。串行推进：配置模型是引擎的输入，引擎是检测器的载体。

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| N-03 | Runtime 配置模型扩展（`kind` / `node_script`，SCHEMA_V18） | P0 | ✅ | N-02 | [N-03-runtime-config-kind.md](./N-03-runtime-config-kind.md) |
| N-04 | `LaunchPlan::Script` 与 NodeBuildEngine | P0 | ✅ | N-01, N-03 | [N-04-script-launch-plan-engine.md](./N-04-script-launch-plan-engine.md) |
| N-05 | 启动检测器策略化与端口探测 | P0 | ✅ | N-04 | [N-05-launch-detectors.md](./N-05-launch-detectors.md) |

### Phase 2 · UI 与端到端验收（P0/P1，2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| N-06 | 前端 UI 接入（Wizard 类型选择 / Dashboard 降级 / api） | P0 | ✅ | N-03, N-04 | [N-06-frontend-ui.md](./N-06-frontend-ui.md) |
| N-07 | 端到端验收与文档收尾（Windows + macOS 闭环） | P1 | ⬜ | N-05, N-06 | [N-07-e2e-acceptance.md](./N-07-e2e-acceptance.md) |

### Phase 3 · 增强与展望（P2，2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| N-08 | 包管理器增强与显式安装（pnpm/yarn 执行链 / `node_install` / 注册表） | P2 | ✅ | N-05 | [N-08-package-manager-enhancement.md](./N-08-package-manager-enhancement.md) |
| N-09 | 展望：monorepo / bun / watch 联动 / 模板 | P2 | ⬜ | N-08 + 触发条件（设计文档 §7 P3） | [N-09-future-extensions.md](./N-09-future-extensions.md) |

---

## 关键依赖链

```text
N-01 工具链检测 ────────────────┐
                                ├──► N-04 LaunchPlan/引擎 ──► N-05 检测器 ──┬──► N-07 端到端验收
N-02 发现/索引 ──► N-03 配置扩展 ┘                            │             │
                                └────────────────────────────► N-06 UI ────┘
N-05 ──► N-08 包管理器增强 ──► N-09 展望（条件触发）
```

- **N-01 / N-02 可并行**；Phase 1 串行（N-03 → N-04 → N-05）。
- N-06 与 N-05 无依赖关系，但 N-07 验收需要两者都完成。
- N-09 为**条件触发**：无真实需求不启动，触发条件见 spec。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录。
3. 新增/调整任务时，重新编号并同步依赖字段。
4. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
5. 全局横切约束统一记录在 `00-全局开发约束.md`；各任务文档的「架构/性能注意点」只写该任务特有内容，与全局约束叠加，不重复。
6. 设计文档 `docs/node-frontend-runtime-design.md` 是单一事实来源；任务 spec 与之冲突时，先改设计文档或在 spec 中显式说明原因与边界。
