# PAF-16 terminal store 监听注册失败永久锁死

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 模拟中途 listen 失败后，重新打开面板可恢复注册
- [ ] 终端功能在部分监听失败时降级可用
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
