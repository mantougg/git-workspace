# F-26 Node 前端项目启动多端口展示/停止不完整

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-02 用户反馈（第 2 项） |
| 关联任务 | N-01 / N-10（Node 工具链）、R-10（Runtime 启停）、R-16（端口管理） |

## 问题描述

前端项目用 yarn serve 一类命令启动时，可能会拉起多个监听端口，但 Runtime
总览里的表格有时完全不显示端口，有时只显示一个。用户还担心停止时是不是只停了
一部分，没把同一个应用带出来的派生进程和端口一起清掉。

## 定位线索

- src-tauri/src/runtime/launch/manager/output.rs 的 Node 分支只提取首个端口
- src-tauri/src/runtime/launch/manager/monitor.rs 只在启动宽限期内记录端口
- src/views/RuntimeDashboard.vue 的 Applications / Processes 表格都直接消费
  process.ports
- 停止链路走 runtime/launch/manager/control.rs + process/kill_tree.rs

## 修复范围

- [ ] 让 Node 启动过程能记录并持久化多个端口
- [ ] 校验 Stop / Restart / Environment Stop 是否完整覆盖同一 runtime 的派生进程
- [ ] Applications 与 Processes 两个表格的端口展示口径一致
- [ ] 增加真实 Node dev server / 多端口 fixture 回归

## 验收标准

- [ ] 多端口 Node 应用能完整展示所有监听端口
- [ ] Stop 后相关端口真实释放，不遗留可运行进程
- [ ] 刷新后进程记录仍与端口信息一致
- [ ] 相关测试通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 修复完成，多端口收集与停止回归通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ⬜ | 用户反馈录入：Node dev server 多端口展示/停止链路需核查 |
| 2026-09-02 | 🟦 | 开始修复：补充多端口解析，取消 Node 端口收集仅限启动宽限期的限制 |
| 2026-09-02 | ✅ | 修复完成：Node 逐行收集全部本地 URL 端口并持久化，新增多端口解析与 Manager 集成回归；真实 Node Vite 停止链路沿用进程树释放机制。验证：manager output 3 passed、Node 多端口集成 1 passed、pnpm build 通过、局部 rustfmt --check 通过。全量 node 测试中的既有 Vite 5176 连接环境失败未纳入本任务 |
