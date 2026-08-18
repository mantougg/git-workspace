# R-08 Runtime Benchmark 与性能基线

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-01 Maven 项目发现与 POM 解析](./R-01-maven-discovery.md)；Benchmark 设施复用 T-07。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置，贯穿全程） |
| 状态 | ⬜ 未开始 |
| 依赖 | R-01, T-07 |
| 对应源文档 | §96 性能 Benchmark、§97 Benchmark 指标、§98 对比测试、§99 性能目标、§4.2 资源目标 |

## 目标

建立 Runtime Workspace 专属 Benchmark 体系：可合成的多模块/多仓库测试工程、全链路耗时与资源指标采集、与 IDEA 的对比框架——后续所有性能相关验收以本任务实测数据为准。

## 需求范围

- [ ] 合成测试工程生成器：10 / 50 / 100 modules × 10 / 50 / 100 repositories 矩阵（§96），含跨 repo 源码依赖拓扑
- [ ] 指标采集（§97）：Project Discovery / POM Parse / Dependency Resolve / Runtime Closure / Synthetic Reactor Generation / Build / Application Start 耗时；Idle Memory / Peak Memory / CPU / Disk IO / Process Count / Thread Count
- [ ] 对比测试框架（§98）：IDEA vs GitWorkspace Runtime，场景含冷启动 / 热启动 / 首次 Build / 二次 Build / 修改单模块 / 修改底层模块 / 多服务启动
- [ ] §99 性能目标校验项自动化：Discovery < 500ms / POM Cache < 50ms / Graph Cache < 100ms / Config Load < 50ms / File Change → Detection < 300ms
- [ ] 接入 T-07 benchmark 设施与报告格式，结果落盘可追踪趋势
- [ ] 各阶段入口回归：后续每完成一个性能敏感任务（R-09 / R-17 / R-18…）跑对比并记录

## 架构 / 性能注意点

- Build 与 Spring Boot 启动时间**不设固定 SLA**，记录趋势与基线即可（§99）。
- 合成工程生成必须确定性（固定随机种子），保证跨次运行可对比。
- Benchmark 自身开销不得计入被测指标；进程级指标采样用独立线程。
- IDEA 对比项允许半自动（人工操作 + 计时脚本），但测量口径必须写入报告。

## 验收标准

- [ ] benchmark 可一键运行并输出完整报告（耗时 + 资源 + 对比）
- [ ] 合成工程矩阵可生成、可被 R-01/R-02 正常发现解析
- [ ] §99 目标项有明确 pass/fail 判定输出
- [ ] 基线报告存档，后续任务可引用对比

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 合成工程生成器（确定性）
- [ ] 耗时指标采集（接入 R-01 起各阶段埋点）
- [ ] 资源指标采样
- [ ] §99 目标校验与报告
- [ ] IDEA 对比流程与口径文档
- [ ] 基线运行与存档
