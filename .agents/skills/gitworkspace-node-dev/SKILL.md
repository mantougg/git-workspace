---
name: gitworkspace-node-dev
description: GitWorkspace Node 前端工程启动任务流程：如何读 docs/tasks-node/ 文档（总索引/全局约束/任务spec）开始与继续 Node 前端工程（N-XX）任务开发、并同步进度。
---

# GitWorkspace Node 前端工程启动任务开发流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-node/` 的任务文档**开始开发**或**继续开发**某个 Node 前端工程启动（N-XX）任务。

这套任务是**功能扩展**（设计来源：`docs/node-frontend-runtime-design.md`）：在既有 Runtime 引擎上新增第二条技术栈——发现 `package.json`、解析 `scripts`、以 `npm/pnpm/yarn run <script>` 启动并纳入统一进程管理。它与重构任务（B-XX）的区别是：**验收靠新功能闭环，但同时不得改写 Maven/Spring Boot 既有链路语义**（设计文档 §2.1 是零改动复用区清单）。

## 文档地图

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/node-frontend-runtime-design.md` | 设计文档：方案与决策的**单一事实来源**（§ 号被各 spec 引用） | 对设计有疑问、spec 与它冲突时 |
| `docs/tasks-node/README.md` | 总索引：9 个任务的阶段/优先级/状态/依赖总表 + 依赖链 + MVP 口径 + 维护规范 | 选任务、核对状态、同步进度时 |
| `docs/tasks-node/00-全局开发约束.md` | Node 特有横切约束（不实现 script 语义 / 禁止自动 install / 可执行检测 / 错误扩展声明 / 纯函数检测器 / 测试策略 / 代码落点） | 任何 N-XX 任务开发前**必读** |
| `docs/tasks-node/N-XX-*.md` | 任务 spec：目标 / 需求范围 / 架构性能注意点 / 验收标准 / 进度 | 开发目标任务时 |
| `docs/tasks-runtime/00-全局开发约束.md` + 根 `AGENTS.md` 平台规范 | Runtime 引擎全局约束与 Windows/macOS/Linux 硬规则，**全文生效** | 任何 N-XX 任务开发前**必读** |

## 关键边界（贯穿所有 N-XX 任务）

1. **不实现 npm script 语义**：只做「检测可执行 + 拼命令 + 复用进程管理」；script 内容交包管理器执行，不解读、不改写。
2. **禁止自动 `npm install`**（网络行为）：仅 N-08 显式 `node_install`；`node_modules` 缺失只给可行动提示。
3. **Windows 可执行检测硬规则**：一律 `find_in_path`（`.exe → .cmd → .bat`），`.cmd` 执行必走 `cmd /C`（`needs_cmd_c`）；禁止 `Command::new("npm")` 裸名兜底。
4. **检测器/决策链/解析全部纯函数**，样例取真实工具输出原文；依赖真实 node 的测试探测不到就 skip 并打印原因。
5. **向后兼容**：配置缺省 `kind=springBoot`，历史配置零迁移；springBoot 链路（banner/端口正则/向导）回归不变是每条验收的隐含项。
6. **DB 迁移顺序**：N-02 = SCHEMA_V17（`node_projects`），N-03 = SCHEMA_V18（`runtime_projects.kind`），不并号。

## 任务地图速查

- **N-01 / N-02**（Phase 0，P0，**可并行**）：Node 工具链检测与决策链；package.json 发现与索引（V17 + `node_list_projects`）。
- **N-03 → N-04 → N-05**（Phase 1，P0，**串行**）：配置 `kind` 扩展（V18）；`LaunchPlan::Script` + NodeBuildEngine；检测器策略化与端口探测。
- **N-06**（Phase 2，P0）：Wizard / Dashboard / api 接入（依赖 N-03、N-04）。
- **N-07**（Phase 2，P1）：双平台端到端验收与文档收尾（依赖 N-05、N-06）；完成后 MVP 可交付。
- **N-08**（Phase 3，P2）：pnpm/yarn 执行链、显式 `node_install`、注册表。
- **N-09**（Phase 3，P2，**条件触发**）：monorepo / bun / watch 联动 / 模板；无真实需求不启动。

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选「依赖均已就绪」的任务；Phase 1 严格串行）。
2. 读 `README.md` 总表，确认状态、优先级、依赖；读 `00-全局开发约束.md` + `tasks-runtime/00-全局开发约束.md`（必读）。
3. 读目标任务文档顶部「**开发前必读**」与设计文档对应章节——**只读这几份**。
4. 通读目标任务文档：目标、需求范围 checklist、验收标准。
5. 状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」），时间线追加「开始开发」。
6. 对要修改的既有符号（如 `LaunchPlan`、`engine_for`、`launcher.rs` 的 match）先跑 GitNexus `impact`，向用户报告 blast radius 后开始（根 AGENTS.md GitNexus 规则）。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」：状态 + 时间线最后一条 + 子任务勾选情况。
2. 核对 README 总表状态一致（不一致以任务文档为准并修正 README）。
3. 从时间线最后一条恢复上下文，继续未勾选子任务。

## 完成一个任务

1. 逐条核对「验收标准」**全部满足**；每条都隐含 springBoot 回归不变。
2. 跑四件套（`cargo fmt --check` / `check` / `test` / `clippy -D warnings`，带 `--manifest-path src-tauri/Cargo.toml`）；动前端再跑 `pnpm build`；动 IPC 契约必须重新生成 golden（`GW_UPDATE_GOLDEN=1`）并跑 `detect_changes()` 核对影响范围。
3. 任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）；README 总表同步 `→ ✅` 并更新「总体进度」计数。
4. 提示用户可开始的下游任务；N-07 完成即 MVP 可交付。

## 必须遵守

- **全局约束优先**：两份 00 约束 + 根 AGENTS.md 平台规范是硬约束；spec「架构/性能注意点」是叠加约束；冲突时在 spec 显式说明原因与边界。
- **进度与状态规则以 README 为准**：进度两处同步、状态流转权威定义在 README 末尾「维护规范」。
- **设计文档是单一事实来源**：spec 与 `docs/node-frontend-runtime-design.md` 冲突时，先改设计文档或在 spec 显式说明，不静默偏离。
- **不扩大改动面**：Maven/Spring Boot 链路只加分支不改语义；发现需要改既有行为时停下来核对设计文档。
- **代码落点**：后端新模块在 `src-tauri/src/node/`，Runtime 内部改动按 00 约束 §7 表落位；前端在 `src/api/` 与 `src/views/`。
