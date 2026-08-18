# R-24 Docker / Kubernetes Runtime

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 扩展运行时 |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | R-10 |
| 对应源文档 | §85 Runtime 与 Docker、§6 P2 功能范围（Docker / Kubernetes Runtime） |

## 目标

把 Runtime Application 抽象为统一运行模型（Local JVM / Docker Container），先落地 Docker 形态：容器内构建/运行 Spring Boot 应用，日志与进程管理语义对齐本地运行。

## 需求范围

- [ ] 统一模型（§85）：`Runtime Application → Local JVM | Docker Container`，配置侧选择运行形态
- [ ] Docker 检测（daemon 可用性 / 版本）与 `DockerNotFound` 可行动错误
- [ ] 镜像构建：基于 Dockerfile 或 Spring Boot 分层构建（`bootBuildImage`），产物缓存策略明确
- [ ] 容器生命周期：create / start / stop / restart / logs / 端口映射，与 R-10 状态机对齐
- [ ] 日志对接 R-11（容器输出 → 同一日志引擎与脱敏管道）
- [ ] Kubernetes：仅做部署目标调研 + 最小可行实现（如生成 manifests 或对接受管集群），范围以调研结论为准并记入时间线

## 架构 / 性能注意点

- Docker 操作一律驱动 Docker CLI / Engine API，不自建容器运行时。
- 镜像构建耗时长，必须走任务队列（T-05）+ 进度事件，不阻塞 UI。
- 容器内进程指标采集口径与本地不同，Dashboard 展示要有来源区分。
- K8s 部分先做调研文档，经确认后再排实现，避免范围失控。

## 验收标准

- [ ] 样例应用以 Docker 形态完成 构建 → 启动 → 日志 → 停止 闭环
- [ ] 本地/容器两种形态在同一 Dashboard 统一管理
- [ ] Docker 不可用时错误可行动
- [ ] K8s 调研结论落档

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 统一运行模型抽象
- [ ] Docker 检测与错误
- [ ] 镜像构建流水线
- [ ] 容器生命周期管理
- [ ] 日志/指标对接
- [ ] K8s 调研与最小实现
