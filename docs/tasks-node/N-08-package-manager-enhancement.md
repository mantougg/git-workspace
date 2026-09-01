# N-08 包管理器增强与显式安装

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md) §2（禁止自动 install）/ §3（可执行检测）；设计文档 [§7 P2](../node-frontend-runtime-design.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 增强与展望 |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | N-05 |
| 对应设计文档 | §7 P2 增强 |

## 目标

补齐 pnpm/yarn 的一等执行支持，提供用户显式触发的依赖安装动作与自定义工具链路径登记。

## 需求范围

- [x] pnpm/yarn 执行链细节：`pnpm <script>`（可省略 run）等命令形状差异；`yarn <script>`；参数透传差异单测
- [x] 显式 `node_install` IPC：任务队列执行（T-05 进度/取消）、首次确认（对齐 §75）、输出走日志引擎；**仍禁止任何自动触发**
- [x] Node 注册表：仿 `jdks` / `maven_executables` 表 + `commands/jdk.rs` 同款 IPC，支持登记自定义 node/pm 路径，决策链最优先读取
- [x] 显式端口字段（可选）：配置加 `node_port`，port_preflight 优先读取

## 架构 / 性能注意点

- install 是长任务 + 网络行为：必须走任务队列（进度可见、可取消），错误可识别可降级（网络/镜像源提示）。
- 注册表与 PATH 检测的优先级冲突时以注册表为准，UI 展示来源（注册表 / PATH）。
- 本任务不做 monorepo / bun / watch 联动（N-09）。

## 验收标准

- [x] pnpm / yarn 样例工程启动闭环（各自真实环境，无环境 skip）
- [x] `node_install` 进度可见、可取消、首次确认、失败错误可行动
- [x] 注册表登记/生效/删除全链路；优先级正确
- [x] 四件套全绿（预存失败已逐一归因，见时间线 2026-09-01 条目）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-09-01

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发；工具链齐备（node 22 + npm/pnpm/yarn），从子任务 1（pnpm/yarn 命令形状纯函数 + 单测）起步 |
| 2026-08-31 | 🟦 | 子任务 1 完成：新增 `node/command.rs` 纯函数 `build_run_args`（npm 用 `--` 分隔透传、pnpm/yarn/bun 直接透传），pipeline 接入替换硬编码 `["run", script, "--", ...]`；6 个单测通过（`cargo test node::command`）。fmt/clippy 全绿项为环境预存问题（rustfmt 1.9.0 与仓库全局不一致、51 处既有 clippy 警告），我方改动文件零新增告警 |
| 2026-08-31 | 🟦 | 子任务 2/3/4 后端完成（commit d4e010d）：`node_install` IPC（confirmed=false 返回 `NodeInstallConfirmationRequired` 结构化确认错误 + `TaskType::NodeInstall` 任务队列执行，runtime 级超时/取消，流式 `node_install_output` 事件 + 完成时 `git_command_result` 汇总，bun 拒绝、超时/失败错误带镜像源提示）；Node 注册表（`node_executables` 表 + list/add/validate/remove/prune 五个 IPC + `resolve_package_manager_with_registry` 决策链最优先读取）；显式端口字段 `node_port`（`port_preflight::explicit_node_ports`） |
| 2026-09-01 | 🟦 | 前端 UI 完成：Dashboard Node 行新增「装依赖」按钮（首次调用拿确认错误 → dialog 展示命令预览 → 确认后提交任务并打开任务面板，§75 对齐）；新增「Node 工具链」设置页（`NodeToolchainView.vue` 仿 JdkManagerView：注册/复检/删除/清理失效），路由 `/node-toolchain` + SideNav 入口；`TaskType` TS 联合补 `nodeInstall` 变体 |
| 2026-09-01 | ✅ | 测试与四件套验证完成，任务关闭。新增 pnpm/yarn 真实启动闭环测试（`node_engine_launch_loopback`：真实 pnpm run dev / yarn run dev spawn + 输出断言，本机工具链未 skip）、pnpm/yarn install 真实冒烟（空依赖工程本地安装 + node_modules 断言）；修复 IPC golden 两处缺口（`TaskType::NodeInstall` sample 缺失 + node.ts 无分号 union 被 golden 解析器误吞，注册 NodeExecutable/NodeExecutableRequest/NodeInstallRequest，`GW_UPDATE_GOLDEN=1` 重新生成）；node 模块 fmt/clippy 遗留清零。四件套：fmt（node 相关全清，仓库其余为 rustfmt 版本预存差异）、clippy（改动文件零告警）、test（node 相关 48 全绿；全量中 5 个失败逐一归因为预存环境/时序问题——maven settings ×2 本机 `~/.m2/settings.xml` 有 localRepository 未隔离、vite 启动探测/日志聚合时序敏感，已用 git stash 验证 HEAD 同样失败）、`pnpm build` 通过 |

### 子任务清单

- [x] pnpm/yarn 命令形状与单测
- [x] `node_install` IPC + 任务队列接入（后端；前端 UI 入口见下一项）
- [x] Node 注册表（表 + IPC + 决策链接入）
- [x] 显式端口字段（可选）
- [x] 前端 UI：安装入口（首次确认 + 输出/进度）与注册表管理入口
- [x] 测试与四件套验证
