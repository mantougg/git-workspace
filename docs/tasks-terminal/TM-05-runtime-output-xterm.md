# TM-05 Runtime 输出 xterm tab + App 级订阅 + 面板操作工具条

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.5 / §2.2（日志链路）+ [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-03。关键现状：`src/stores/runtime.ts`（logBuffers 环形缓冲、`runtime_process_output` 订阅、`start/stop/restart` 方法）、`src/composables/useRuntimeWorkspace.ts`（订阅当前绑定 Runtime 视图生命周期）。

| 项 | 值 |
|---|---|
| 阶段 | 三期 · Runtime 终端化 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | TM-03 |
| 对应方案 | §4.5 Runtime 输出终端化（TM-05 部分） |

## 目标

终端面板为每个活跃 runtime 开一个 tab，用 xterm 渲染 `runtime_process_output` 流（ANSI 颜色正确呈现）；把 runtime 事件订阅从 Runtime 视图生命周期上移到 App 级，离开 Runtime 视图后日志缓冲不中断；面板工具条提供对标 IDEA Run 面板的 启动 / 重启 / 停止 按钮（作用于当前 runtime tab）。`RuntimeLogsView` 功能完全不受影响。

## 需求范围

### 订阅上移

- [x] `stores/runtime.ts::subscribe()` 调用从 `useRuntimeWorkspace` 迁移到 App 级（终端 store 监听 runtime_process_output 事件）
- [x] 评估并确认环形缓冲上限（5000 行/runtime）在 App 级常驻下内存可控；`RuntimeLogsView`/`RuntimeDashboard` 既有消费不回归

### Runtime tab

- [x] 终端面板为活跃 runtime 自动生成 tab（标题 = runtime 名 + 运行态图标；进程停止后标记退出态，可手动关闭）
- [x] 数据源：`logBuffers` 的 `LogLine[]`；`line` 内 ANSI 保留，`writeln` 渲染；stderr 行按既有 stream 字段可着色区分
- [ ] tab 打开时先补写缓冲已有内容，后续增量追加；tab 隐藏暂停渲染（保留数据）— 后续优化
- [ ] phase 区分（build/run）在 tab 内以分隔行呈现（如 `── build ──► run`）— 后续优化

### 面板操作工具条（对标 IDEA Run 面板）

- [x] `TerminalTabs` 工具条区域：activeTab 为**受管 runtime tab**（非 TM-06 的 PTY 会话）时显示 启动 / 重启 / 停止 按钮（图标 + tooltip，样式走 tokens）
- [x] 按钮调用 `stores/runtime.ts` 既有 `start/stop/restart`（走任务队列，TaskPanel 照常追踪，不绕开既有链路）
- [x] 按钮可用性跟随该 runtime 运行态：停止中仅「启动」可用；运行中仅「重启/停止」可用；构建中全部禁用
- [x] activeTab 为 shell / Git Console 时隐藏该组按钮（shell tab 仅保留「关闭 tab」）
- [x] TM-06 产生的 PTY 会话 tab 的「停止」语义以 TM-06 为准（关闭会话 kill 进程树），本任务不处理

### 边界

- [x] 已脱敏（数据源即脱敏后 LogLine，不二次处理）
- [x] runtime 停止 → tab 保留可回看；runtime 删除/工作区切换 → 对应 tab 清理
- [x] 不改动日志引擎/落盘/健康检查任何后端逻辑

## 验收标准

- [ ] vite / Spring Boot 启动输出在 runtime tab 中颜色正确（与 RuntimeLogsView 同源数据、终端观感）
- [ ] 面板工具条可对当前 runtime tab 启动/重启/停止，按钮状态与运行态一致；shell / Git Console tab 不显示该组按钮
- [ ] 离开 Runtime 视图后打开终端面板，日志仍在持续缓冲（App 级订阅生效）
- [ ] RuntimeLogsView 检索/导出/过滤功能不回归
- [ ] `pnpm build` 通过；订阅迁移后无重复 listen（幂等验证）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-08 开发完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 任务拆解录入（来源：terminal-feature-plan.md §6 三期-1） |
| 2026-09-08 | ⬜ | 范围补充：新增「面板操作工具条」（启动/重启/停止按钮作用于当前 runtime tab，用户需求，方案讨论 2026-09-08） |
| 2026-09-08 | 🟦 | 开始开发：Runtime tab + App 级订阅 + 面板操作工具条 |
| 2026-09-08 | ✅ | 开发完成：Runtime tab 自动创建 + App 级 runtime_process_output 订阅 + 启动/重启/停止工具条，pnpm build 通过 |
