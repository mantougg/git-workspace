# R-14 Runtime 安全与错误处理

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、[R-11 Runtime 日志引擎](./R-11-log-engine.md)；Secret 能力复用 T-08。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环（P0 收尾，横切） |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-10, R-11, T-08 |
| 对应源文档 | §74 Security、§75 Command Safety、§76 Environment Security、§77 Log Secret Mask、§78 项目状态安全、§79 错误分类、§80 用户错误提示 |

## 目标

把 Runtime 链路的安全护栏与错误体验补齐到产品级：统一错误分类、可行动错误提示、命令执行确认、敏感信息掩码、用户项目只读护栏。

## 需求范围

- [x] 错误分类全集落地（§79）：`ProjectNotFound / MavenNotFound / JdkNotFound / InvalidPom / DependencyResolveFailed / SourceMappingFailed / BuildFailed / ProcessStartFailed / PortOccupied / HealthCheckFailed / ProcessCrashed`，结构化字段穿透 IPC 到 UI
- [x] 可行动错误提示（§80）：Reason + 上下文（PID / 端口 / 模块）+ Suggested Actions 按钮；禁止只显示 `Process exited with code 1`
- [x] Command Safety（§75）：Pre/Post Build Script 首次执行必须用户确认；默认禁止自动执行 shell script；确认状态持久化
- [x] 环境变量敏感 key（§76）：`PASSWORD / TOKEN / SECRET / PRIVATE_KEY / API_KEY` 模式匹配，UI 掩码 `••••••••`；与 R-07 配置、R-11 日志脱敏打通
- [x] 项目状态安全护栏（§78）：运行链路中对用户 pom / 源码 / git branch 的写操作断言（开发期 assertion + 代码评审清单）
- [x] 端口占用错误（`PortOccupied`）带占用进程信息，Suggested Actions 联动 R-16 能力（未交付前显示信息即可）

## 架构 / 性能注意点

- 错误类型与 Git 侧 `GitWorkspaceError` 体系对齐（T-08），新增 Runtime 分类而非另立体系。
- 脱敏/掩码规则**单一实现**，日志、配置 IPC、UI 三处复用同一规则集。
- 确认类交互（脚本执行 / Force Kill）状态可撤销（「不再询问」可重置）。
- 护栏断言只加在 Runtime 写路径，不影响正常只读流程性能。

## 验收标准

- [x] §79 每类错误都有触发样例、结构化字段与对应文案（`error.rs` 全集测试 + 结构化变体 details 断言）
- [x] 端口占用场景错误含占用方 PID/进程名与建议动作（`PortOccupied` details 带 pid/processName；netstat/tasklist、lsof//proc 跨平台解析）
- [x] 未确认的 Pre/Post Script 不执行；确认后执行且记录（`script-approvals.json` 持久化 + lastExecutedAt；单测覆盖确认/拒绝/失败/内容变更重确认）
- [x] 敏感变量在 UI / 日志 / IPC 返回三处均掩码（测试断言：`secret.rs` 规则 + `config.rs` redact/preserve 往返 + `sensitive_environment_values_are_masked_in_stream`）
- [x] 全程无对用户 pom / 源码的写操作（护栏断言 + 代码走查：`runtime/guard.rs` 校验所有写路径在 `.gitworkspace/` 下，接入 reactor/config/logs/classpath 四处）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-26 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-26 | 🟦 开始开发 | 启动 R-14：核对 §79 错误分类在 error.rs 的现状、T-08 脱敏复用、Command Safety 确认通道与端口占用检测的落点，补齐产品级错误体验与安全护栏 |
| 2026-08-26 | ✅ 完成 | 落地：① `error.rs` 补 `InvalidPom / PortOccupied / HealthCheckFailed / ScriptConfirmationRequired / ScriptFailed` 五类（code + recoverable + 结构化 details）+ §79 全集测试；② `process/port.rs` 端口占用检测（Windows netstat/tasklist、Unix lsof//proc，解析函数纯函数可测）；③ `launch/port_preflight.rs` 启动前端口预检（显式端口 bind 探测 → PortOccupied，接入 manager.prepare）；④ Command Safety：config 增 `pre/post_build_script`（serde default 兼容），`runtime/script_approval.rs`（app data `script-approvals.json` 持久化、内容哈希重确认、lastExecutedAt 记录、可重置），三个 IPC（get/approve/reset），R-09 流水线 6.5/8.5 步执行脚本（未确认 → ScriptConfirmationRequired；`cmd /C` / `sh -c` + 5min 超时 + 脱敏转发 + 前缀标记）；⑤ `runtime/guard.rs` 只读护栏（debug_assert + Permission 错误，接入 reactor/config/logs/classpath 四处写路径）；⑥ 前端 `RuntimeErrorAlert.vue`（code→文案 + 上下文 + Suggested Actions，脚本确认对话框联动重试）、Dashboard 错误横幅 + 脚本确认管理卡片（可重置）、向导脚本字段。验证：`cargo test` 380 passed / 32 failed（32 全为基线既有的本机环境性失败——stash 基线对照一致，R-14 新增 21 个测试全绿，含真实 `cmd /C` 脚本执行流与端口 bind 探测）；golden `ipc_golden` 2/2 绿；`pnpm build`（vue-tsc + vite build）绿 |
| 2026-08-26 | ✅ 修复 | 全量测试失败清零攻坚（此前 32 个环境性失败逐一定位，实为 7 处问题，其中 **5 处真实产品 bug**）：① `find_root_project`（pipeline.rs）路径字符串相等比较被 Windows 混合分隔符（`\Temp\...\repo/app/pom.xml`）破坏 → 归一化比较（R-02 `path_key` 存正斜杠，用户配置可能是反斜杠/混合）；② `service.rs find_project`（R-13 引入，inspect/closure 同病）；③ `exec_resolve` 增量 diff 的 `known_paths` 同病（第二次解析误发 project_discovered）；④ `manager.infer_main_class` 同病（R-06 mainClass 推断失败）；⑤ `java/detect.rs find_in_path` 缺 PATHEXT 语义——先命中 mise 的 Unix `mvn` sh 脚本（error 193）而漏掉可执行的 `mvn.cmd` → Windows 上 Maven 检测永远失败；⑥ 测试断言 `bound_jdk` 分隔符不敏感；⑦ real-maven 集成 fixture 不绑 JDK，构建（JAVA_HOME=17）与启动（系统 java 8）版本错配 → boot_fixture 注册并绑定 JAVA_HOME。验证：`JAVA_HOME=temurin-17 + cargo test` **411 passed / 1 failed / 2 ignored**（唯一失败 benchmark smoke 并行资源冲突，单独跑通过）——含真实 mvn 集成（Synthetic Reactor 构建、真实 Spring Boot Start→Running→端口探测→Stop 全闭环） |

### 子任务清单

- [x] Runtime 错误分类与结构化字段
- [x] 可行动错误提示组件与文案
- [x] Command Safety 确认机制
- [x] 敏感变量掩码统一规则与三处接入
- [x] 只读护栏断言
- [x] 单元测试（错误 / 脱敏 / 确认流）
