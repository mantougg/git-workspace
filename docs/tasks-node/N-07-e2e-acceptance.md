# N-07 端到端验收与文档收尾

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§8 / §9](../node-frontend-runtime-design.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · UI 与端到端验收 |
| 优先级 | P1 |
| 状态 | 🟦 进行中 |
| 依赖 | N-05, N-06 |
| 对应设计文档 | §8 验收标准（MVP）、§9 风险与开放问题 |

## 目标

用真实样例把 MVP 闭环在双平台验收一遍，回归安全项，同步文档；本任务完成后 Node MVP 整体可交付。

## 需求范围

- [x] Vite 样例工程闭环：发现 → 建配置 → 启动 → 端口正确识别 → 日志流 → 停止——已固化为自动化集成测试 `real_vite_project_full_loop_with_port_release`（真实 `npm create vite` 产物 + 真实 install + 真实启动 + 端口释放断言），Linux 实测通过；Windows/macOS 真机复核待补
- [x] **进程树回归**：发现并修复真实缺陷——unix 上 SIGTERM 先杀死 npm（npm 不等待 vite 退出），vite 被 reparent 到 init 且继续持有输出管道，parent 链 `kill_tree` 对「父死孙活」失效，Stop 后端口不释放；修复：launch 子进程 `process_group(0)` 独立成组 + `killpg` 组信号（`launcher.rs` / `kill_tree.rs`），Windows 维持原路径（root `cmd /C` 存活期间 parent 链完整）。E2E 断言 Stop 后端口真实释放
- [x] 错误路径验收：`node_modules` 缺失 / script 不存在 / pm 缺失 → 三类可行动提示（`node_engine_reports_missing_dependencies_without_installing`、`ScriptNotFound` 校验含 `availableScripts` 结构化字段、`bun_decision_is_actionable_error_not_executable`）
- [x] Secret 脱敏回归：日志 / preview / UI 三处共用 `is_sensitive_environment_key`（`core::secret` 8 测试 + `logs::redact` 4 测试 + IPC 不回传敏感值），全绿
- [x] 旧 springBoot 配置全链路回归：真实 Spring Boot 闭环测试（启动 → Running → Stop → JVM 消失）3 通过 1 manual ignored（含原因），验证进程组改动对 Java 链路无回归
- [x] 文档收尾：根 `AGENTS.md` 平台规范补「npm.cmd 必须 find_in_path + cmd /C」与「父死孙活进程组规则」参照实现条目；设计文档 §5/§8/§9 更新；README 总表收尾

## 架构 / 性能注意点

- 验收样例固定为真实 Vite 模板工程（`npm create vite` 产物，入 fixtures 或临时目录，不进用户项目）。
- 所有验收结论附验证命令与输出摘要，记入时间线（可审计）。
- 发现 MVP 外缺陷：不随手修，登记到 N-08/N-09 或 docs/tasks-fix/。

## 验收标准

- [x] 设计文档 §8「验收标准（MVP）」7 条逐项核对通过（平台注记见 §8 勾选项）
- [~] 双平台验收记录（命令 + 结果）入时间线：Linux 完整闭环已记录；Windows/macOS 真机闭环受本开发环境限制待补（测试均已固化，真机只需跑 `cargo test`）
- [x] AGENTS.md 增补条目与设计文档状态更新完成
- [x] 四件套 + 前端构建全绿（全量 `cargo test` 745 通过 0 失败；改动文件 fmt/clippy 清零，仓库其余为预存基线；`pnpm build` 通过）

## 进度

### 状态

- 当前状态：进行中（本机可验证项全部完成，待 Windows/macOS 真机复核后关闭）
- 最近更新：2026-09-02

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发。本机为 Linux（node 22.22.3 + npm/pnpm/yarn）：先跑全量测试建立基线，修复预存失败使四件套全绿，再以真实 Vite 样例在本机做闭环验收；Windows/macOS 真机闭环按实际可达性记录结论 |
| 2026-09-02 | 🟦 | 测试隔离修复：全量基线 741 过 / 2 败——`maven::settings` 两个测试未与真实 `~/.m2/settings.xml`（本机带 `localRepository`）隔离。重构 `resolve_local_repository` 为「纯函数核心 + 薄包装」（`resolve_local_repository_from` 显式注入用户级/全局级路径），测试全部改走注入路径，新增 `user_settings_beats_global_settings` 覆盖优先级；GitNexus impact LOW（4 个直接调用方，公共签名不变）。settings.rs fmt 清零 |
| 2026-09-02 | 🟦 | 新增 N-07 核心 E2E：`real_vite_project_full_loop_with_port_release`（`manager/tests.rs::real_node_vite`，与 `real_maven` Spring Boot 真实闭环对称）——真实 `npm create vite@latest --template vanilla` + `npm install`（node/npm/网络不可达即 skip 打印原因；测试内 spawn 遵守 find_in_path 硬规则解析 npm 绝对路径）→ `discover_package_jsons`/`sync_node_projects` 发现断言 → `create_config(kind=Node)` → SystemLaunchRunner 真实启动 → `run_strategy=NodeScript` + `command_preview` 落库断言 → 探测端口真实可连 → 日志流含 VITE 横幅 → Stop → **端口真实释放**轮询断言 + 生命周期链断言 |
| 2026-09-02 | 🟦 | **E2E 揪出真实产品缺陷（设计文档 §9 风险实锤）**：首跑 Stop 后端口不释放、残留 vite 孤儿进程。根因：unix 上 SIGTERM 杀死 npm 后 vite 被 reparent 到 init，parent 链 kill_tree 找不到孙子；且 vite 继续持有输出管道使 monitor 永不收口。手工复现实锤（`kill -TERM <npm>` 后 vite 仍 LISTEN）。修复（GitNexus impact：kill_process_tree HIGH 9 个直接调用方已全量回归 / launch_command LOW 4 个）：`launch_command` 对 unix launch 子进程 `process_group(0)` 独立成组；`terminate_process` 优雅停止、`kill_process_tree` 升级强杀均先走 `killpg` 组信号（新增 unix `libc` 依赖），parent 链遍历保留兜底 adopted 等非组长场景；Windows 行为不变。修复后 E2E 通过（46.7s） |
| 2026-09-02 | 🟦 | 回归与四件套：全量 `cargo test` **745 通过 0 失败**（含真实 Spring Boot 闭环 `classpath_run_full_cycle_with_real_spring_boot_app` 等 3 通过，证明进程组改动对 Java 链路无回归；kill_tree/streaming/manager 真实进程测试全绿）；改动文件（settings/kill_tree/launcher/manager tests）fmt 与 clippy 清零（顺手修 tests.rs 一处预存 `manual_inspect`），仓库其余为预存基线（N-08 口径）；`pnpm build` 通过；ipc_golden 2 测试通过（本次无 IPC 契约变更，无需重新生成） |
| 2026-09-02 | 🟦 | 文档收尾：AGENTS.md §2 增补「npm/pnpm/yarn 包管理器 find_in_path + cmd /C」参照实现、§3 增补「父死孙活进程组规则」；设计文档 §5 进程树终止机制修订（unix 进程组 + killpg，替代「无平台分支新增」原表述）、§8 验收标准 7 条打勾（附平台注记）、§9 风险条目记录实锤与修复；本表与 README 同步 |

### 子任务清单

- [x] 闭环验收自动化（真实 Vite 工程 E2E：发现→配置→启动→端口→日志→停止+端口释放）——Linux 实测通过
- [ ] Windows 真机复核（含进程树/端口释放：跑 `cargo test real_vite_project_full_loop` + `real_process_windows`）
- [ ] macOS 真机复核（同上，另跑 `real_maven` Spring Boot 闭环）
- [x] 错误路径与脱敏回归（既有测试逐项核对全绿）
- [x] springBoot 回归（真实闭环测试通过，进程组改动无回归）
- [x] 文档收尾（AGENTS.md / 设计文档 / README）
