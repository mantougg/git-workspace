# T-32 插件系统 / Scheduled Tasks（Automation Platform）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-23 Workspace Pipeline](./T-23-pipeline.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 6 · Automation Platform（P3） |
| 优先级 | P3 |
| 状态 | ✅ 已完成 |
| 依赖 | T-23 |
| 对应 Roadmap | §73 Phase 6 Automation Platform、§74 P3 |

## 目标

落地 Automation Platform 的开放能力：插件系统与定时任务，让用户扩展自定义动作与自动化。

## 需求范围

- [x] 插件系统：自定义 Actions / Scripts / 插件加载与隔离
- [x] Scheduled Tasks：定时执行任务 / Pipeline
- [x] Task Templates：任务与 Pipeline 模板库
- [x] 与 T-23 Pipeline / T-24 DAG 复用执行内核

## 架构 / 性能注意点

- 插件沙箱与权限边界需明确（P3，可先做脚本级动作而非任意原生插件）。
- 定时任务调度器与主任务队列分离，避免阻塞交互；执行复用 T-24 DAG 与 §45 限流。

## 验收标准

- [x] 用户可注册自定义脚本动作并在 Pipeline 中复用
- [x] Scheduled Tasks 按计划触发且可暂停/删除
- [x] 模板可保存、导入、复用

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ✅ | 完成。①脚本动作（P3 边界内的脚本级插件）：schema V21 `plugin_actions` 表 + CRUD + `run_plugin_action`（跨平台 cmd /C、sh -c，cwd 按 scope=仓库根/工作区根，超时可配 + CREATE_NO_WINDOW + 分离流读线程）；②Scheduled Tasks：`scheduled_tasks` 表（interval / daily HH:MM 两种调度）+ 独立 30s 轮询线程（setup 期 spawn，独立于主任务队列），到期任务派生线程执行——脚本动作直接跑、pipeline 复用 T-23 `compile_pipeline` + T-24 `submit_dag` 执行内核，运行后推进 next_run（失败也推进防风暴），支持暂停/恢复/删除；③模板库：T-23 已有保存/复用，本任务补导入/导出（JSON 落盘/读入，导入分配新 id + validate）；④前端 `api/automation.ts` + 「自动化」视图（/automation，Git 分组，三 Tab：脚本动作/定时任务/模板导入导出）。边界说明：脚本级动作而非原生插件（任务文档允许），动作命令为用户自建故运行即显式触发。验证：`cargo test --lib` 810 通过（automation 3 项：interval/daily 调度计算含 DST 日历日断言、非法调度拒绝）；`pnpm build` 通过。存量偶发说明：全量并发下 `flood_is_aggregated` / `runtime_benchmark_smoke` 等计时敏感测试偶发失败（每次跑失败项不同、单跑全过、文件与 T-32 无交集） |

### 子任务清单

- [x] 插件/脚本动作注册机制
- [x] Scheduled Tasks 调度
- [x] 模板库
