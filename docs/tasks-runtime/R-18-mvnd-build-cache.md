# R-18 构建加速：mvnd 与构建缓存分级

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-05 Maven 检测与执行策略](./R-05-maven-detection.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-09, R-05 |
| 对应源文档 | §20 Maven Daemon、§73 Build Cache Strategy、§67 性能优化 |

## 目标

在 Build Engine 抽象之上接入 Maven Daemon（mvnd），并按 §73 路线落地构建缓存分级，让频繁 Build / Restart 场景获得可测量的加速。

## 需求范围

- [ ] mvnd 检测（安装路径 / 版本）与 Settings UI：`Build Engine ○ Maven ● Maven Daemon`（§20）
- [ ] mvnd 执行接入 R-05 Executor 抽象（命令构造 / 输出转发 / 取消）
- [ ] 缓存分级路线（§73）：第一阶段 Maven Native Cache（已在 R-09）→ 第二阶段 **Runtime Dependency Cache**（本任务实现：模块输入指纹未变则跳过重构建）→ 第三阶段 Content Hash Cache（仅评估与设计，不实现）
- [ ] mvnd 收益测量：频繁 Build / Restart 场景对比（用 R-08 设施产出数据）
- [ ] mvnd daemon 生命周期：闲置退出策略、异常状态识别与回退普通 mvn

## 架构 / 性能注意点

- mvnd 是可选增强：未安装/异常时必须无感回退 mvn，不构成硬依赖。
- Runtime Dependency Cache 的输入指纹 = 模块源码 hash + pom hash + 上游产物 hash；指纹设计要防误判（宁可重建不错过）。
- **不自行实现 Java 编译缓存**（全局约束 §5）：缓存粒度到「模块是否重构建」为止。
- mvnd daemon 常驻内存计入资源预算，闲置超时回收。

## 验收标准

- [ ] mvnd 模式构建功能正确，且对比 mvn 有可量化收益（R-08 报告）
- [ ] 未变化模块在二次构建中被跳过（日志可证）
- [ ] mvnd 缺失/异常时自动回退 mvn 并提示
- [ ] daemon 闲置回收策略生效

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] mvnd 检测与设置项
- [ ] mvnd Executor 接入
- [ ] Runtime Dependency Cache（输入指纹）
- [ ] daemon 生命周期管理
- [ ] 收益 Benchmark（R-08）
- [ ] 单元/集成测试
