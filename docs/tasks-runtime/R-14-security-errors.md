# R-14 Runtime 安全与错误处理

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、[R-11 Runtime 日志引擎](./R-11-log-engine.md)；Secret 能力复用 T-08。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环（P0 收尾，横切） |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-10, R-11, T-08 |
| 对应源文档 | §74 Security、§75 Command Safety、§76 Environment Security、§77 Log Secret Mask、§78 项目状态安全、§79 错误分类、§80 用户错误提示 |

## 目标

把 Runtime 链路的安全护栏与错误体验补齐到产品级：统一错误分类、可行动错误提示、命令执行确认、敏感信息掩码、用户项目只读护栏。

## 需求范围

- [ ] 错误分类全集落地（§79）：`ProjectNotFound / MavenNotFound / JdkNotFound / InvalidPom / DependencyResolveFailed / SourceMappingFailed / BuildFailed / ProcessStartFailed / PortOccupied / HealthCheckFailed / ProcessCrashed`，结构化字段穿透 IPC 到 UI
- [ ] 可行动错误提示（§80）：Reason + 上下文（PID / 端口 / 模块）+ Suggested Actions 按钮；禁止只显示 `Process exited with code 1`
- [ ] Command Safety（§75）：Pre/Post Build Script 首次执行必须用户确认；默认禁止自动执行 shell script；确认状态持久化
- [ ] 环境变量敏感 key（§76）：`PASSWORD / TOKEN / SECRET / PRIVATE_KEY / API_KEY` 模式匹配，UI 掩码 `••••••••`；与 R-07 配置、R-11 日志脱敏打通
- [ ] 项目状态安全护栏（§78）：运行链路中对用户 pom / 源码 / git branch 的写操作断言（开发期 assertion + 代码评审清单）
- [ ] 端口占用错误（`PortOccupied`）带占用进程信息，Suggested Actions 联动 R-16 能力（未交付前显示信息即可）

## 架构 / 性能注意点

- 错误类型与 Git 侧 `GitWorkspaceError` 体系对齐（T-08），新增 Runtime 分类而非另立体系。
- 脱敏/掩码规则**单一实现**，日志、配置 IPC、UI 三处复用同一规则集。
- 确认类交互（脚本执行 / Force Kill）状态可撤销（「不再询问」可重置）。
- 护栏断言只加在 Runtime 写路径，不影响正常只读流程性能。

## 验收标准

- [ ] §79 每类错误都有触发样例、结构化字段与对应文案
- [ ] 端口占用场景错误含占用方 PID/进程名与建议动作
- [ ] 未确认的 Pre/Post Script 不执行；确认后执行且记录
- [ ] 敏感变量在 UI / 日志 / IPC 返回三处均掩码（测试断言）
- [ ] 全程无对用户 pom / 源码的写操作（护栏断言 + 代码走查）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Runtime 错误分类与结构化字段
- [ ] 可行动错误提示组件与文案
- [ ] Command Safety 确认机制
- [ ] 敏感变量掩码统一规则与三处接入
- [ ] 只读护栏断言
- [ ] 单元测试（错误 / 脱敏 / 确认流）
