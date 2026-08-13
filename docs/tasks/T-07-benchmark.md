# T-07 Benchmark 系统（提前到 Phase 0）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-01 Scanner](./T-01-scanner.md)、[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | T-01, T-02 |
| 对应 Roadmap | §64 Benchmark 系统、§65 性能目标、§66 测试体系 |

## 目标

建立独立 Benchmark 系统，用可复现的真实数据校准 Roadmap G4 与 §65 的全部性能目标。**提前到 Phase 0**：否则「100 仓库 <2s / 500 仓库 <8s」等目标始终无法验证。

## 需求范围

- [x] 合成仓库生成器：`generate_repos`（N 仓库，每仓库 3 commit + 文件），数量可调
- [x] 每组测试：Initial Scan / Status Refresh 已实现（File Watch / Branch / Graph / Search / Batch 剩余）
- [x] 指标采集：Time 已实现（CPU / Memory / Disk IO / Thread / IPC / Git Process 剩余）
- [x] 结果输出：文本/Markdown 报告（JSON + 历史对比剩余）
- [ ] 回归门槛：接入 CI（剩余）

## 架构 / 性能注意点

- Benchmark 与业务代码解耦，用独立二进制 / feature 门控，不进入主应用热路径。
- 合成仓库要足够「真实」（多 branch、多 commit、合理文件量），否则测不出真实瓶颈。
- 记录 Git Process Count 用于验证 §45 并发限流是否真的生效（防止 fork 爆炸）。

## 验收标准

- [ ] 可一键生成 100 / 500 / 1000 仓库并跑完全部测试组
- [ ] 产出含 Time / Memory / Git Process 的结构化报告
- [ ] 校准出 §65 各性能目标的真实基线值，并回写各任务验收标准
- [ ] 100 仓库初始扫描 < 2s、500 仓库 < 8s 目标经过实测确认或修正

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：benchmark 模块（generate_repos + scan/status 测量 + 报告）+ binary 入口；实测 10 仓库：scan 349ms（debug）、per-repo status 9.8ms。剩余：更多测试组、CPU/Memory 等指标、历史对比、CI

### 子任务清单

- [x] 实现合成仓库生成器
- [x] 实现各测试组与指标采集（scan/status + Time）
- [ ] 实现报告与历史对比（剩余）
- [x] 建立性能基线（10 仓库：scan 349ms、per-repo 9.8ms）
- [ ] 接入 CI 回归门槛（剩余）
