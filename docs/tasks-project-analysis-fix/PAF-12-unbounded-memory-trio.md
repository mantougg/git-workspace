# PAF-12 无界内存增长三连（AI gateway records / 终端 writeBuffer / chat known_addrs）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-11/P1-12/P1-13，主控亲自验证 |
| 关联任务 | AI-02（Gateway）、TM-02（终端面板）、LAN chat |

## 问题描述

三处实证的无界增长：

1. **AI gateway 请求记录**：`ai/gateway.rs:161`
   `records: Mutex<HashMap<String, RequestRecord>>` 只有插入/读取，
   `prune_terminal`（:756-761）定义后**全仓零调用**（grep 实证）；每条记录
   克隆完整 `AiRequest`（含全部消息正文），长会话重度使用即内存泄漏。
2. **终端隐藏面板写缓冲**：`src/stores/terminal.ts` 6 处
   `writeBuffer.push`（:193/:237/:277/:292/:316/:329）无任何上限/丢弃；
   面板 `v-if` 卸载 XtermView 后所有 PTY/runtime 输出无限堆积（对照
   runtime store logBuffers 有 5000 行环形上限）；`pauseSession/flushBuffer`
   （:531-544）全工程零调用。
3. **LAN chat 地址表**：`chat/manager.rs:72`
   `known_addrs: Mutex<HashSet<SocketAddr>>` 只插入（:334/:587/:800）无淘汰。

## 定位与修复建议

1. gateway：接入 prune_terminal（终态记录按容量/年龄清理），长会话验证；
2. writeBuffer：加环形上限（对照 runtime logBuffers 5000 行），超出丢弃
   最旧；或面板关闭时彻底停用缓冲；
3. known_addrs：LRU/TTL 淘汰。

## 验收标准

- [ ] 三处内存均有界（可写容量断言测试）
- [ ] 长跑 runtime + 隐藏终端面板场景内存不无限增长
- [ ] 网关/终端既有行为不回归

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
