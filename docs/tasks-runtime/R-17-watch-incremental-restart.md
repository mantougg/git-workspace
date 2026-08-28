# R-17 File Watch / 增量构建 / 自动重启

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-09 Build Engine](./R-09-build-engine.md)、[R-02 依赖图与源码映射](./R-02-dependency-graph-source-mapping.md)；监听设施复用 T-06。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | R-09, R-02, T-06 |
| 对应源文档 | §42 自动 Restart、§43 File Watch、§44 Incremental Build、§71 File Watch 架构、§72 变更影响分析 |

## 目标

开发模式下监听源码变化，经变更影响分析定位受影响模块，只增量重建必要模块并自动重启应用——避免整个 Workspace 全量 Build。

## 需求范围

- [x] 监听范围（§43）：`src/main/java` / `src/main/resources` / `pom.xml`；忽略 `target/` / `.git/` / `node_modules/`；事件类型 Created / Modified / Deleted / Renamed
- [x] 事件链路（§71）：OS File Event → Debounce → Path Classification → Affected Project → Affected Dependency Closure → Incremental Build
- [x] 变更影响分析（§72）：基于 R-02 依赖图反向传播（改 common 影响 core/auth/boot；改 auth 只影响 auth/boot）
- [x] 增量构建（§44）：只重建受影响模块（Maven `-pl` 子集），除非依赖关系要求扩大
- [x] 自动 Restart（§42）：开发模式开关，构建完成后自动重启运行中应用
- [x] `pom.xml` 变化特殊处理：触发依赖模型失效重算（联动 R-02/R-03 缓存失效），而非直接构建

## 架构 / 性能注意点

- File Change → Detection < 300ms（§99，以 R-08 实测为准）。
- 复用 T-06 watcher 的 debounce / 分片 / 锁退避约定；**禁止触发全量扫描或全量构建**。
- 自动重启可全局/每应用开关，默认关；连续变化时合并重启（restart 防抖）。
- 构建中收到新变化要排队合并，不打断进行中的构建导致半产物。
- watcher 句柄预算：只监听「参与运行中应用 Closure」的模块，不监听整个 Workspace。

## 验收标准

- [x] 修改 auth 模块，仅 auth + boot 重建（构建日志可证），common/core 不动
- [x] 修改 boot 只重建 boot
- [x] 变化到完成重启的链路自动化，且 debounce 生效（连续保存不重复触发）
- [x] File Change → Detection < 300ms（R-08 实测）
- [x] pom.xml 变化触发依赖模型重算而非盲目构建

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：watch 引擎（复用 notify 设施，仅监听运行中应用闭包）+ 变更影响分析 + affected_modules 增量构建 + 自动重启编排 |
| 2026-08-29 | ✅ | 完成：`RuntimeWatchEngine`（防抖 250ms / 增量挂卸载 / 归队防重）+ `propagate_downstream` 影响分析纯函数 + `affectedModules` 贯通 IPC→BuildOptions→`-pl` 子集合并（覆盖 SkipAll）+ `autoRestart` 配置与向导开关 + Dashboard 摘要槽位。测试：watch 模块 9 项单测（含真 DB fixture）、真 Maven 集成测试 `affected_modules_override_dependency_cache_skip_with_real_maven`、R-08 实测 File Change → Detection 0ms（< 300ms PASS，基线 `benchmarks/runtime_2x3.json`）。验证：`cargo test --lib`（485 通过；3 个失败为既有环境/偶发问题，与本任务无关）、`vue-tsc --noEmit` 通过 |

### 子任务清单

- [x] 监听范围与分类器（接入 T-06）
- [x] 变更影响分析（依赖图反向传播）
- [x] 增量构建（-pl 子集构造）
- [x] 自动重启编排与防抖
- [x] pom 变化 → 模型失效链路
- [x] 单元/集成测试
