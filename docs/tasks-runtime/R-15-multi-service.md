# R-15 Multi-Service Runtime 与 Runtime Environment

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、[R-13 Runtime UI](./R-13-runtime-ui.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | 🟦 进行中 |
| 依赖 | R-10, R-13 |
| 对应源文档 | §38 Multi-Service Runtime、§39 Service Dependency、§40 Parallel Start、§82 Runtime Environment、§84 多服务环境模板 |

## 目标

支持多服务一键启动：定义 Runtime Environment（服务集合 + 各自配置），按服务依赖关系自动排序、无依赖服务并行启动，服务于联调/演示场景。

## 需求范围

- [ ] Runtime Environment 模型（§82）：Local / Development / Test / Demo，每环境含服务列表及各服务的 JDK / Profile / 环境变量 / 端口 / 外部服务备注
- [ ] 环境配置持久化：`.gitworkspace/environments/<name>.json`（§84，可 Git 版本化共享）
- [ ] 一键 `Start Environment` / `Stop Environment`（§38）
- [ ] Service Dependency（§39）：服务间依赖声明（如 gateway → auth → system → common），拓扑排序决定启动顺序
- [ ] Parallel Start（§40）：无依赖关系的服务并行启动，有依赖的严格串行
- [ ] 部分失败语义：某服务启动失败不阻塞无依赖分支；整体状态汇总可见
- [ ] 启动/停止走 R-12 Task Scheduler 统一限流与进度展示

## 架构 / 性能注意点

- 服务依赖图与用户声明分离：依赖关系存环境配置，拓扑排序运行时计算，环依赖报错。
- 并行度受全局并发预算约束（Build = 2 等），多服务启动是排队调度而非无脑并发。
- 服务间「就绪等待」第一版用固定顺序 + 延迟/轮询占位，健康检查就绪门限归 R-16 后接入。
- 环境内服务可引用已有 Runtime 配置，环境只存覆盖项（避免配置双份漂移）。

## 验收标准

- [ ] gateway → auth → system → common 依赖链按拓扑序启动
- [ ] 无依赖服务（auth / system / file）并行启动，总耗时有收益
- [ ] 单服务失败不影响无依赖分支，环境状态正确汇总
- [ ] 环境 JSON 可保存/加载/团队共享
- [ ] Stop Environment 全部进程正确终止

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] Environment 模型与持久化
- [ ] 服务依赖声明与拓扑排序
- [ ] Start/Stop Environment 编排（含并行）
- [ ] 部分失败语义与状态汇总
- [ ] UI（环境列表 / 编辑 / 一键启停）
- [ ] 单元/集成测试
