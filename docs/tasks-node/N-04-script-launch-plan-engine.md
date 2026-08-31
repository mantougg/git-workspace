# N-04 LaunchPlan::Script 与 NodeBuildEngine

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§4.4 / §4.5](../node-frontend-runtime-design.md)；根 `AGENTS.md` 平台兼容规范 §2/§3（cmd /C、子进程）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 配置与启动闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | N-01, N-03 |
| 对应设计文档 | §4.4 LaunchPlan 扩展、§4.5 BuildEngine 接入 |

## 目标

`engine_for` 接入 `"node"`：NodeBuildEngine 直通产出 `LaunchPlan::Script`，launcher 新增 Script 分支拼出 `npm run <script>` 命令，复用既有进程托管注入与预览落库。

## 需求范围

- [ ] `LaunchPlan::Script { executable, args, env, working_dir, preview }` 变体（`runtime/build/mod.rs:133`，内部类型不进 golden）
- [ ] `engine_for("node")` → `NodeBuildEngine`；未知 id 错误文案与单测同步更新
- [ ] `NodeBuildEngine.build`：校验（node/pm 经 N-01 决策链解析、script 存在）→ env 合并（复用五层）→ Pre/Post Script（复用 `script_approval.rs` 确认制）→ 产出 plan；**不触碰** Maven 段（依赖图/Closure/Reactor/Classpath），`execute_build` 按 engine 分叉
- [ ] `launcher.rs` 三分支补齐：`launch_command`（`needs_cmd_c` 为真包 `cmd /C`）、`plan_preview`、`plan_working_dir`；托管标记 env 注入不变
- [ ] `args` 组装：`["run", <script>]` + 有 `program_arguments` 时追加 `["--", ...]`
- [ ] `node_modules` 缺失检测 → 可行动提示（不自动 install，00 约束 §2）
- [ ] `runtime_processes.command_preview` 落库（§75 可追溯）

## 架构 / 性能注意点

- MVP 只执行 npm：决策链命中 pnpm/yarn 时若可执行，仍按 `<pm> run <script>` 同一代码路径执行（命令形状一致）；不可执行 → `PackageManagerNotFound`。pnpm 参数的细微差异（如 `pnpm dev` 省略 run）属 N-08 优化，本任务统一 `run`。
- `Script` 变体的命令行短，无需超长命令设施（F-11 pathing jar 为 Java 专属）。
- Restart 的 `skip_build` 缓存语义对 Script 原样成立（plan 缓存复用）。

## 验收标准

- [ ] `engine_for("node")` 返回可用引擎；`engine_for_rejects_unknown_ids_actionably` 测试更新
- [ ] `launch_command` 单测：Windows 分支命令为 `cmd /C <npm.cmd 路径> run dev`（`cfg(windows)` 断言，路径归一化后断言）；Unix 为直 spawn
- [ ] 真实集成：fixture package.json（`dev` script 打印一行后退出）启动→日志可见输出→进程退出状态正确（无 node 环境 skip 并打印原因）
- [ ] preview 落库可查；`node_modules` 缺失提示可行动
- [ ] 四件套全绿

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发：按 N-03 RuntimeKind 与 node_projects 索引实施独立 NodeBuildEngine、Script LaunchPlan 和 launcher 分支。 |
| 2026-08-31 | ✅ | 完成：新增 NodeScript 策略与 LaunchPlan::Script，NodeBuildEngine 直通校验工具链/脚本/依赖目录并合并环境，launcher 支持 Windows cmd /C 与进程托管；真实 npm fixture 输出与退出码验证通过，Maven 调用数为 0。全量测试 699/711 通过，12 项为既有 AI snapshot/JDK8-Maven 环境基线失败；前端类型检查通过。 |

### 子任务清单

- [x] LaunchPlan::Script 变体 + launcher 三分支
- [x] NodeBuildEngine 直通 + execute_build 分叉
- [x] cmd /C 包装与 env 注入
- [x] node_modules 缺失检测提示
- [x] 集成测试与四件套验证（Node 专项通过；全量基线问题已记录）
