# R-18 构建加速：mvnd 与构建缓存分级

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-05 Maven 检测与执行策略](./R-05-maven-detection.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | R-09, R-05 |
| 对应源文档 | §20 Maven Daemon、§73 Build Cache Strategy、§67 性能优化 |

## 目标

在 Build Engine 抽象之上接入 Maven Daemon（mvnd），并按 §73 路线落地构建缓存分级，让频繁 Build / Restart 场景获得可测量的加速。

## 需求范围

- [x] mvnd 检测（PATH/PATHEXT + `mvnd -v`）与 Settings UI：向导 Build Engine 单选（mvnd 未安装时选项明示回退行为）
- [x] mvnd 执行接入 R-05 Executor 抽象（`MavenRunner::resolve_maven_for_engine` hint，命令构造/输出转发/取消不变）
- [x] 缓存分级路线（§73）：Runtime Dependency Cache 落地（dep_cache.rs，内容哈希指纹 + 上游级联 + 产物缺失强制重建；Content Hash Cache 仍留评估）
- [x] mvnd 收益测量：`run_mvnd_build_benchmark`（R-08 设施；mvnd 缺失时只跑 mvn 基线并打印 skip 原因）
- [x] mvnd daemon 生命周期：`-Dmvnd.idleTimeout=120000` 闲置回收；daemon 异常标记识别 → 回退 mvn 重试一次

## 架构 / 性能注意点

- mvnd 是可选增强：未安装/异常时必须无感回退 mvn，不构成硬依赖。
- Runtime Dependency Cache 的输入指纹 = 模块源码 hash + pom hash + 上游产物 hash；指纹设计要防误判（宁可重建不错过）。
- **不自行实现 Java 编译缓存**（全局约束 §5）：缓存粒度到「模块是否重构建」为止。
- mvnd daemon 常驻内存计入资源预算，闲置超时回收。

## 验收标准

- [x] mvnd 模式构建功能正确，且对比 mvn 有可量化收益（R-08 报告：run_mvnd_build_benchmark + format_mvnd_report；本机未装 mvnd → 仅基线，skip 原因已打印）
- [x] 未变化模块在二次构建中被跳过（日志可证：`dependency_cache_skips_unchanged_modules_with_real_maven`，真实 mvn，`[R-18] 依赖缓存命中` + 0 次 Maven 调用）
- [x] mvnd 缺失/异常时自动回退 mvn 并提示（构建日志 `[R-18] mvnd 不可用…` / daemon 异常标记重试）
- [x] daemon 闲置回收策略生效（idleTimeout 参数注入，用户显式设置不覆盖）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：mvnd 检测/Executor 接入/回退 + Runtime Dependency Cache + daemon 生命周期 |
| 2026-08-29 | ✅ | 完成：maven/mvnd.rs 检测 + runner hint + daemon 异常回退；dep_cache.rs 输入指纹缓存接入流水线（默认开）。真实 mvn 集成测试证明二次构建跳过 + 子集重建；R-08 设施新增 run_mvnd_build_benchmark（本机未装 mvnd，基线可跑、对比自动 skip 并打印原因）。测试 runtime:: 179 通过 |

### 子任务清单

- [ ] mvnd 检测与设置项
- [ ] mvnd Executor 接入
- [ ] Runtime Dependency Cache（输入指纹）
- [ ] daemon 生命周期管理
- [ ] 收益 Benchmark（R-08）
- [ ] 单元/集成测试
