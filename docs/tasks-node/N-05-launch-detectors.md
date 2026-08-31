# N-05 启动检测器策略化与端口探测

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md) §5（检测器纯函数）；设计文档 [§4.6](../node-frontend-runtime-design.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 配置与启动闭环 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | N-04 |
| 对应设计文档 | §4.6 启动成功与端口检测策略化 |

## 目标

把 Spring 硬编码的 banner/端口检测改为按 `kind` 策略化；Node 侧以「宽限期存活即 Running + 通用 URL 正则探端口」覆盖主流 dev server。

## 需求范围

- [ ] 检测器抽象：monitor 按配置 `kind` 选择检测器集；springBoot 集保持现状（banner `Started \S+ in [\d.]+ seconds` + `started on port` 正则，回归不变）
- [ ] Node 集：**无 banner**（宽限期 `start_grace` 到且进程存活即 Running）；端口探测用通用 URL 正则 `https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d+)`
- [ ] URL 探测规则：取**首个 localhost URL** 且仅在宽限期内采纳（防误报，设计文档 §9）
- [ ] 端口预检（`launch/port_preflight.rs`）Node 侧解析：`program_arguments` 的 `--port` / `-p`、`environment` 的 `PORT`；均缺省跳过预检
- [ ] 检测器与解析全部纯函数，样例单测

## 架构 / 性能注意点

- 样例必须取自真实工具输出原文（Vite `Local:   http://localhost:5173/`、webpack `Project is running at http://localhost:8080/`、Next `- Local: http://localhost:3000`），不凭记忆编造。
- 「编译失败但不退进程」语义不引入新状态：进程活着即 Running，错误靠日志呈现。
- 正则编译进 `LazyLock`/once 缓存（沿用 output.rs 现有模式），不为每行日志重复编译。

## 验收标准

- [ ] 单测：Vite / webpack / Next 三份真实输出样例各识别出正确端口
- [ ] 单测：无 URL 输出不误报；非 localhost URL 不采纳；宽限期外 URL 不采纳
- [ ] 单测：`--port 3000` / `-p 3000` / `PORT=3000` 三种显式端口解析
- [ ] 回归：springBoot 样例输出检测结果与改动前一致
- [ ] 真实集成：Vite 工程启动 → 端口正确落 `ports_json` 并发事件（无 node 环境 skip）
- [ ] 四件套全绿

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 检测器按 kind 策略化抽象
- [ ] 通用 URL 端口正则 + 采纳规则
- [ ] Node 端口预检解析
- [ ] 样例单测 + springBoot 回归
- [ ] 真实集成与四件套验证
