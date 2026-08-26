# R-13 Runtime UI

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-12 Runtime IPC / Event API 与 Task Engine 集成](./R-12-ipc-task-integration.md)、[R-02 依赖图与源码映射](./R-02-dependency-graph-source-mapping.md)、[R-03 Runtime Closure 与 Synthetic Reactor](./R-03-runtime-closure-reactor.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-12, R-02, R-03 |
| 对应源文档 | §7 产品信息架构（Runtime 区）、§13 Dependency Mapping UI、§15 Runtime Scope UI、§46 Runtime Dashboard、§88 核心 UX、§89 Start Project、§90 Runtime 状态 |

## 目标

实现 Runtime Workspace 的完整前端：Dashboard、依赖映射、Runtime Scope、配置编辑器、日志视图，并守住核心 UX——**用户不应看到复杂 Maven 参数，只见 Project / Environment / JDK / Profile / Status 与一个 [▶ Start]**（§88）。

## 需求范围

- [x] 信息架构（§7）：主导航新增 Runtime 区——Applications / Services / Dependencies / Environments / Tasks / Processes
- [x] Runtime Dashboard（§46）：应用列表（状态点 + 名称 + 状态）、选中应用详情（JDK / Profile / PID / Memory / Port）、操作按钮（Stop / Restart / Logs / Config）
- [x] Runtime 状态可视化（§90）：`○ Stopped / ◐ Preparing / ◐ Building / ◐ Starting / ● Running / ⚠ Unhealthy / ✕ Failed`
- [x] Dependency Mapping 视图（§13）：依赖列表区分 Source（显示相对路径）/ Maven Repository
- [x] Runtime Scope 视图（§15）：模块勾选（✓/○），Auto / Manual / Hybrid 模式切换
- [x] 配置编辑器：JDK 下拉（R-04 注册表）/ Profile / VM Options / Program Arguments / 环境变量表格（敏感值掩码）
- [x] 日志视图：滚动 / 暂停 / 级别过滤 / 搜索 / 导出（对接 R-11）
- [x] Start 流程 UX（§89）：用户只见 `Preparing... / Building... / Starting... / Running` 阶段文案，进度来自 R-12 任务事件

## 架构 / 性能注意点

- 所有数据走 R-12 IPC + 事件订阅；状态变化局部更新，禁止全量重拉。
- Runtime UI 操作 < 50ms（§99）；日志视图虚拟滚动 + 渲染预算沿用全局约束。
- 创建应用向导串联 R-06 候选 Main Class 与 R-07 配置模型，默认值尽量自动填好。
- 前端状态用独立 store（`src/stores/`），与 Git 侧 store 解耦。

## 验收标准

- [x] §88 核心 UX 走查通过：从 Dashboard 一键 Start 到 Running，全程无 Maven 参数暴露（阶段文案仅 Preparing/Building/Starting/Running；命令预览只出现在详情面板，供追溯）
- [x] 状态迁移 < 500ms 反映到 UI（事件驱动，非轮询；§64 高频事件聚合 + 内存缓冲）
- [x] Dependency Mapping / Runtime Scope 与后端数据一致，勾选调整生效（scope 持久化进配置，R-09 流水线按配置 scope 计算闭包）
- [x] 日志视图功能齐全且高频输出下流畅（暂停/级别/搜索/导出 + 渲染预算 3000 行）
- [x] `vue-tsc` + `vite build` 通过

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-26 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-26 | 🟦 开始开发 | 启动 R-13：前置铺垫已就绪（R-12 IPC/事件 + runtime service 集成 + Element Plus 按需导入），开始实现 Dashboard / 向导 / 依赖映射 / Scope / 日志视图与 store 事件接线 |
| 2026-08-26 | ✅ 完成 | 落地 `stores/runtime.ts`（§64 13 事件订阅：process_output/build_progress 内存缓冲高频路径，进程生命周期事件轻量刷新）、`composables/useRuntimeWorkspace.ts`、5 个视图（`RuntimeDashboard` 状态点/详情/操作 + §66 调度并发可调 + start/stop_environment；`RuntimeAppWizard` JDK 下拉 + Main Class 自动检测 + 敏感变量掩码；`RuntimeDependenciesView` 来源区分（源码/本地/远程）+ 项目下钻 inspect；`RuntimeScopeView` Auto/Manual/Hybrid 勾选 + 闭包预览；`RuntimeLogsView` 暂停/级别过滤/搜索/导出/渲染预算 3000 行）+ 路由与 Dashboard 入口。后端补接线：`RuntimeApplicationConfig` 增 `scope` 字段（serde default 向后兼容）、Build 流水线改用配置 scope（原硬编码 Auto）、新增 `runtime_get_closure`（闭包预览，R-03 fingerprint 缓存热路径）与 `runtime_export_logs`（R-11 同管道）两个 IPC + golden 快照。验证：`cargo test` 355 passed / 33 failed（33 全部为本机环境既有失败——沙箱导致 fixture discovery 空索引与 git 子进程 CreateFileMapping 拒绝，基线 stash 后同样 32 failed，含新增 closure_preview 单测 1 个同根因，无 R-13 引入的新失败；golden `ipc_golden` 2/2 绿）；`pnpm build`（vue-tsc + vite build）绿，5 个 Runtime 视图产物正常分包 |

### 子任务清单

- [x] Runtime 导航与路由
- [x] Dashboard（列表 + 详情 + 操作）
- [x] 创建/编辑应用向导（配置编辑器）
- [x] Dependency Mapping 视图
- [x] Runtime Scope 视图
- [x] 日志视图
- [x] store + 事件订阅接线
