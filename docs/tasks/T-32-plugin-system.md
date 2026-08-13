# T-32 插件系统 / Scheduled Tasks（Automation Platform）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-23 Workspace Pipeline](./T-23-pipeline.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 6 · Automation Platform（P3） |
| 优先级 | P3 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-23 |
| 对应 Roadmap | §73 Phase 6 Automation Platform、§74 P3 |

## 目标

落地 Automation Platform 的开放能力：插件系统与定时任务，让用户扩展自定义动作与自动化。

## 需求范围

- [ ] 插件系统：自定义 Actions / Scripts / 插件加载与隔离
- [ ] Scheduled Tasks：定时执行任务 / Pipeline
- [ ] Task Templates：任务与 Pipeline 模板库
- [ ] 与 T-23 Pipeline / T-24 DAG 复用执行内核

## 架构 / 性能注意点

- 插件沙箱与权限边界需明确（P3，可先做脚本级动作而非任意原生插件）。
- 定时任务调度器与主任务队列分离，避免阻塞交互；执行复用 T-24 DAG 与 §45 限流。

## 验收标准

- [ ] 用户可注册自定义脚本动作并在 Pipeline 中复用
- [ ] Scheduled Tasks 按计划触发且可暂停/删除
- [ ] 模板可保存、导入、复用

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 插件/脚本动作注册机制
- [ ] Scheduled Tasks 调度
- [ ] 模板库
