# R-23 Debug 与 IDE 协同

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 扩展运行时 |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-10 |
| 对应源文档 | §86 Debug、§87 与 IDEA 协同 |

## 目标

为运行中应用提供 JDWP Debug 端口能力，并理顺与 IDE 的协同路径——GitWorkspace 负责起 Debug 端口，**不实现 IDE Debugger**，IDE（IDEA / VS Code）按需 Attach。

## 需求范围

- [ ] Debug 模式开关：启动参数注入 `-agentlib:jdwp=transport=dt_socket,server=y,suspend=n,address=*:5005`（§86）
- [ ] Debug Port 管理：默认端口、冲突检测与自动递增、每应用独立配置
- [ ] `suspend=y/n` 选项（等待调试器连接后再启动）
- [ ] IDE 协同指引（§87）：展示 Attach 所需信息（host/port/命令），提供「Open Source in IDEA/VS Code」入口（复用 T-31 IDE 集成能力）
- [ ] Debug 状态在 Dashboard/详情中可见（端口、是否已附加——仅展示可探测信息）

## 架构 / 性能注意点

- JDWP 只影响启动参数组装（R-10 Launcher），不侵入构建链路。
- Debug 端口与普通端口统一走 R-16 端口冲突检测。
- 不实现任何调试协议（JDI/JDWP 客户端），只做端口与参数管理。

## 验收标准

- [ ] Debug 模式启动的应用带正确 JDWP 参数，端口可连接
- [ ] IDEA / VS Code Attach 到端口后断点可用（人工验证 + 记录）
- [ ] 端口冲突时自动/提示更换
- [ ] `suspend=y` 场景启动等待行为正确

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Debug 配置模型与 JDWP 参数注入
- [ ] Debug Port 管理与冲突检测
- [ ] Attach 信息展示与 IDE 入口
- [ ] Dashboard Debug 状态
- [ ] 测试（参数断言 + Attach 人工验证记录）
