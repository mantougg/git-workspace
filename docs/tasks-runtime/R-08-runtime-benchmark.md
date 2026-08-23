# R-08 Runtime Benchmark 与性能基线

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-01 Maven 项目发现与 POM 解析](./R-01-maven-discovery.md)；Benchmark 设施复用 T-07。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置，贯穿全程） |
| 状态 | 🟦 进行中 |
| 依赖 | R-01, T-07 |
| 对应源文档 | §96 性能 Benchmark、§97 Benchmark 指标、§98 对比测试、§99 性能目标、§4.2 资源目标 |

## 目标

建立 Runtime Workspace 专属 Benchmark 体系：可合成的多模块/多仓库测试工程、全链路耗时与资源指标采集、与 IDEA 的对比框架——后续所有性能相关验收以本任务实测数据为准。

## 需求范围

- [x] 合成测试工程生成器：10 / 50 / 100 modules × 10 / 50 / 100 repositories 矩阵（§96），含跨 repo 源码依赖拓扑
- [x] 指标采集（§97）：Project Discovery / POM Parse / Dependency Resolve / Runtime Closure / Synthetic Reactor Generation 耗时已采集；Build / Application Start 字段预留待 R-09 / R-10 接入；Idle Memory / Peak Memory / CPU / Disk IO / Process Count / Thread Count 已采集
- [x] 对比测试框架（§98）：IDEA vs GitWorkspace Runtime，场景含冷启动 / 热启动 / 首次 Build / 二次 Build / 修改单模块 / 修改底层模块 / 多服务启动——口径文档 `R-08-idea-comparison.md`（半自动，IDEA 侧人工计时）
- [x] §99 性能目标校验项自动化：Discovery < 500ms / POM Cache < 50ms / Graph Cache < 100ms / Config Load < 50ms / File Change → Detection < 300ms
- [x] 接入 T-07 benchmark 设施与报告格式，结果落盘可追踪趋势
- [x] 各阶段入口回归：后续每完成一个性能敏感任务（R-09 / R-17 / R-18…）跑对比并记录——回归方式：`runtime --matrix` + 自动基线对比，基线已存档

## 架构 / 性能注意点

- Build 与 Spring Boot 启动时间**不设固定 SLA**，记录趋势与基线即可（§99）。
- 合成工程生成必须确定性（固定随机种子），保证跨次运行可对比。
- Benchmark 自身开销不得计入被测指标；进程级指标采样用独立线程。
- IDEA 对比项允许半自动（人工操作 + 计时脚本），但测量口径必须写入报告。

## 验收标准

- [x] benchmark 可一键运行并输出完整报告（耗时 + 资源 + 对比）——`cargo run --release --example benchmark -- runtime [repos] [modules] [--json] [--matrix]`
- [x] 合成工程矩阵可生成、可被 R-01/R-02 正常发现解析——§96 九档全矩阵实测 + roundtrip 单测
- [x] §99 目标项有明确 pass/fail 判定输出——五项全部自动判定（实测结果见下方时间线与 `benchmarks/README.md`）
- [x] 基线报告存档，后续任务可引用对比——`docs/tasks-runtime/benchmarks/runtime_*.json`（9 档）+ 对比自动打印

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-21 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-21 | 🟦 开始开发 | 启动 R-08：复用 T-07 benchmark 设施，合成 Maven 工程生成器 + 耗时/资源指标采集 + §99 校验 + 报告 |
| 2026-08-21 | ✅ 完成 | 完成确定性合成工程生成器（§96 矩阵 + 跨 repo 依赖）、全阶段耗时采集（Discovery→Parse→Cache→Index→Graph→Closure→Reactor→Config）、独立线程资源采样（RSS/CPU/线程/磁盘/子进程）、§99 五项 PASS/FAIL 自动判定、Markdown/JSON 报告与基线对比、CI 门槛、IDEA 对比口径文档；§96 九档基线实测存档（`benchmarks/runtime_*.json` + README）。验证：`cargo test` 280 passed / 2 ignored（1 个 R-05 既有环境性失败：本机存在 `~/.m2/settings.xml` 导致 settings 测试假设不成立，与本任务无关）；`cargo clippy --all-targets --all-features` 0 error；真实 `mvn validate` 集成测试通过（mvn 3.9.16）。基线发现：Discovery/Cache 目标在 ≤~1000 POM 达标、PomCache 容量 2048 在大规模下淘汰（详见 benchmarks/README.md） |

### 子任务清单

- [x] 合成工程生成器（确定性）——`src-tauri/src/benchmark/maven_gen.rs`，索引派生内容、无 RNG，两次生成逐字节一致（单测覆盖）
- [x] 耗时指标采集（接入 R-01 起各阶段埋点）——分阶段独立计时：Discovery / POM Parse / POM Cache Hit / Index Sync / Graph / Closure / Reactor / Config / File-Change-Detection；Build/App Start 字段预留待 R-09/R-10
- [x] 资源指标采样——独立线程 20ms 采样：Idle/Peak RSS、CPU 均值、线程峰值（/proc）、磁盘 IO 增量、子进程数（剔除线程伪进程）
- [x] §99 目标校验与报告——`budget_verdicts()` 五项自动 PASS/FAIL + Markdown/JSON + 基线对比
- [x] IDEA 对比流程与口径文档——`R-08-idea-comparison.md`（7 场景口径 + 结果模板，半自动）
- [x] 基线运行与存档——§96 九档全矩阵实测，落盘 `benchmarks/` 随仓库版本化
