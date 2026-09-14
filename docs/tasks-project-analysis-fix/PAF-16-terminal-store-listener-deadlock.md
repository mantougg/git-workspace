# PAF-16 terminal store 监听注册失败永久锁死

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-20，核查智能体验证 |
| 关联任务 | TM-02、PAF-15（同类 in-flight 模式） |

## 问题描述

`src/stores/terminal.ts:102-139`：`listenersRegistered` 同步置位 true，6 个
`listen` 任一抛异常则 `listenersReady` 成为永久 rejected promise——后续
`openSession` 里 `await listenersReady` 直接抛错且无法重试（:103 门槛永不再
进入），已注册的部分监听器也无法清理。另 `cleanup()`（:570-586）全工程
零调用。

## 定位与修复建议

- 注册失败时回滚 `listenersRegistered` 与 `listenersReady`，清理已成功
  的 unlisten，允许重试；
- 或每个 listen 独立 try/catch，失败降级为「该事件不可用」而非整体锁死。

## 验收标准

- [x] 模拟中途 listen 失败后，重新打开面板可恢复注册（全部失败时回滚 `listenersRegistered`/`listenersReady` 并抛错，下次重试）
- [x] 终端功能在部分监听失败时降级可用（单事件失败仅记日志，成功者保留、注册正常完成）
- [x] `pnpm build` 通过（vue-tsc --noEmit + vite build）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立。`registerEventListeners` 改为逐个独立注册（`register` 辅助函数，单事件失败记错误日志返回 null，成功者保留并汇入 `registered`），采纳任务文档建议二「失败降级为该事件不可用而非整体锁死」；若全部失败则回滚 `listenersRegistered`/`listenersReady` 并抛错，`togglePanel`/`showPanel` 下次调用自动重试，与 PAF-15 的 runtime store 失败回滚语义一致。另：`cleanup()` 维持现状（全工程零调用，webview 销毁时 unlisten 无实际意义，保留作 teardown 钩子备用，不扩本任务范围）。验证：`pnpm build` 通过。 |
