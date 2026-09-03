# F-35 Runtime 总览 Processes 表格分页

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-03 用户反馈（第 3 项） |
| 关联任务 | F-26（Processes 表格口径）、R-12（runtime_list_processes） |

## 问题描述

Runtime 总览的 Processes 卡片表格无分页：进程记录含历史运行（每次 Start
新增一行，`runtime_list_processes` 无过滤无上限），表格持续撑高，把下方
「脚本执行确认」「Task Scheduler」面板推到视野外。

## 定位线索

- `src/views/RuntimeDashboard.vue` Processes Panel：
  `n-data-table :data="store.processes"`，未传 `pagination`。
- 排序口径已就绪：后端 `runtime/launch/store.rs::list_processes`
  `ORDER BY id DESC`（最新在前）；前端 `stores/runtime.ts::upsertProcess`
  新记录 `unshift` 头部。**缺的只是分页**，不缺排序。
- naive-ui `n-data-table` 传 `pagination` 对象即启用客户端分页
  （data 传全量数组，组件自己切片）。

## 方案（选定：n-data-table 客户端分页）

1. Processes 表格增加 `pagination` 配置：`page=1`、`pageSize=5`（默认每页
   5 条）、`showSizePicker` + `pageSizes=[5,10,20]`。
2. 展示数据用 computed 兜底排序（`processId` 降序），防事件 upsert 原地
   替换打乱顺序；后端虽已 `ORDER BY id DESC`，前端兜一层口径更稳。
3. 事件驱动新记录到达时：naive-ui 默认保持当前页；处于第 1 页时新记录
   自然可见，非第 1 页不强跳（用户正在翻历史时不打断）。
4. Panel header 增加总数 tag（如 `12 条`），提示被分页收起的记录量。

### 备选（未采纳）

- 「只看运行中」过滤开关：对排查有用，但用户诉求是分页，先不扩范围；
  可作为后续独立小改进。
- 服务端分页：数据量（本地工具，进程记录几十到几百行）不值得引入
  分页 IPC 协议。

## 修复范围

- [x] Processes 表格分页（默认 5 条/页 + size picker）
- [x] 展示 computed 按 processId 降序兜底（sortedProcesses）
- [x] Panel header 总数 tag（"共 N 条"）
- [x] Desktop Skin tokens 合规（分页器用 Naive UI 默认主题，不覆盖样式）

## 验收标准

- [x] 默认每页 5 条，最新记录（processId 最大）在第一页第一行
      （`sortedProcesses` computed 按 processId 降序，pagination 客户端分页）
- [x] 翻页 / 切换每页条数正常；事件推送新记录不报错、不强跳页
      （naive-ui pagination 默认保持当前页，new records unshift to head 由
      sortedProcesses 排序兜底，第 1 页新记录自动可见）
- [x] 表格高度不再随历史记录无限增长，下方面板正常可见
- [x] Applications 表格行为不受影响（用户未要求，不动）
- [x] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-03 修复完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-03 | ⬜ | 用户反馈录入：Processes 表格撑高看不到下方面板，需分页（默认 5 条/页，最新在前） |
| 2026-09-03 | 🟦 | 开始修复 |
| 2026-09-03 | ✅ | 修复完成：n-data-table pagination（默认 5 条/页 + size picker 5/10/20），sortedProcesses computed 按 processId 降序兜底排序，Panel header 加总数 tag。验证：pnpm build 通过 |
