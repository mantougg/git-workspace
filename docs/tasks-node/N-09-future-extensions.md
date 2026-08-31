# N-09 展望：monorepo / bun / watch 联动 / 模板

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§7 P3 / §9](../node-frontend-runtime-design.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 增强与展望 |
| 优先级 | P2 |
| 状态 | ⬜ 未开始（**条件触发**） |
| 依赖 | N-08 + 触发条件 |
| 对应设计文档 | §7 P3 展望、§9 开放问题 |

## 触发条件

**无真实用户需求不启动。** 触发示例：用户工作区出现 npm workspaces monorepo 且需要按子包启动；用户环境以 bun 为主；明确要求前端工程随源码变更自动重启。触发后先把具体场景记入本文件时间线，再拆子任务。

## 目标（候选范围，启动时再裁剪）

- monorepo：npm workspaces / pnpm workspaces 子包发现与 script 路由（根 `package.json` 的 `workspaces` 字段 + 子包 scripts）
- bun：`bun.lockb` 从「只识别」升级为可执行（`bun run <script>`）
- watch 联动：前端工程接入 R-17 File Watch 自动重启（注意 dev server 自带 HMR，联动价值需重新评估）
- 模板：R-19 Runtime Templates 增加 node 类型模板
- 开放问题决策：是否做统一项目视图（Maven/Node 合并列表）；node 配置纳入 R-15 Runtime Environment 分组启停

## 架构 / 性能注意点

- monorepo 发现**复用** `node_projects` 表（子包各占一行），不建新表；路由逻辑放决策链扩展。
- dev server 进程常驻且自带 HMR，R-17 联动默认应为「不自动重启」，避免与 HMR 语义打架。

## 验收标准

- [ ] 启动时按实际触发场景补写

## 进度

### 状态

- 当前状态：未开始（条件触发）
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] （触发后按场景拆解）
