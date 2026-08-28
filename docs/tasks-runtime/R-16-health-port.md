# R-16 Health Check 与 Port Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | 🟦 进行中 |
| 依赖 | R-10 |
| 对应源文档 | §41 Health Check、§81 Port Manager（§80 端口占用错误提示已在 R-14 落地，本任务提供其能力底座） |

## 目标

为运行中应用提供健康状态检测与端口管理能力：识别 Starting / Healthy / Unhealthy，发现端口占用并给出处理手段，并作为多服务编排（R-15）的就绪门限。

## 需求范围

- [ ] Health Check（§41）：检测方式支持 Port / HTTP / TCP / Spring Boot Actuator（`/actuator/health`）
- [ ] 健康状态机：`Starting / Healthy / Unhealthy / Stopped`，变化发 `health_changed` 事件
- [ ] 检查配置：每应用可配端点/端口/间隔/超时；有 actuator 时自动发现
- [ ] Port Manager（§81）：常用端口占用检测，识别占用进程（PID / 进程名）
- [ ] 端口操作：Find Process / Kill Process（确认后）/ Change Runtime Port（改写应用配置）
- [ ] 就绪门限接入 R-15：依赖方等待被依赖服务 Healthy 后启动

## 架构 / 性能注意点

- 健康检查低频轮询 + 指数退避：Unhealthy 不刷请求；进程退出即停检。
- 端口占用识别走 OS 能力（Windows `netstat`/IP Helper，Unix `lsof`/`ss`），解析要有单元测试样例。
- HTTP 检查超时必须短且有上限，不为检查阻塞调度线程。
- Kill 他人进程属危险操作，确认文案明确进程身份（全局约束 §3）。

## 验收标准

- [ ] 带 actuator 的样例应用启动后状态正确流转 Starting → Healthy
- [ ] 无 actuator 应用回退 Port/TCP 检测可用
- [ ] 8080 被外部进程占用时，启动前给出占用方信息并可一键处理
- [ ] 健康检查接入后，R-15 依赖服务等待 Healthy 再启动
- [ ] 进程停止后检查正确收尾，无泄漏轮询

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | 🟦 | 开始开发：健康检查器（Port/TCP/HTTP/Actuator）+ 状态机 + 端口管理（复用 process/port.rs 检测底座） |

### 子任务清单

- [ ] 健康检查器（Port/HTTP/TCP/Actuator）
- [ ] 状态机 + health_changed 事件
- [ ] 端口占用识别（跨平台）
- [ ] Find / Kill / Change Port 操作
- [ ] R-15 就绪门限接入
- [ ] 单元/集成测试
