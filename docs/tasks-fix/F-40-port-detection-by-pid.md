# F-40 端口检测以 PID 归属为主（正则兜底）+ PID tooltip 列出全部 PID

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-04 用户反馈（第 3 项） |
| 关联任务 | F-34（端口归属确权）、F-26（多端口）、F-32（vite8 端口探测） |

## 需求描述

1. Application 端口检测改为根据 PID 推测：周期枚举 OS 监听表
   （`detect_listening_ports`）并按本次启动的进程树 PID 过滤，命中即收为
   应用端口；**日志正则保留为兜底**（候选端口加速首显，仍以树内确权为准）。
   （已与用户确认：PID 为主 + 正则兜底。）
2. PID 展示：主标签 `75784 (+2)` 形式保持不变，悬浮 tooltip 在
   「端口 → PID」映射之外**列出进程树全部 PID 明细**。（已与用户确认。）

## 实现

- `src-tauri/src/runtime/launch/manager/ports.rs`：attribution 线程增加
  周期 PID 扫描——每 ~2s 枚举监听表 + 进程树，树内监听端口直接加入
  ports 口径并记入 confirmed（无需正则先命中）；既有正则候选 → 确权
  流程不变（兜底/加速）。
- `src/views/RuntimeDashboard.vue`：两处 PID 列 render（Applications 表、
  Processes 表）与详情区 PID tooltip 增加「进程树 PID」完整列表行。

## 验收标准

- [x] 日志无端口输出（或非典型 banner）的进程也能经 PID 扫描收出端口
      （attribution 线程每 2s 枚举监听表按进程树过滤直接并入；
      单测 `merge_tree_owned_ports_adds_tree_listeners_without_regex`）
- [x] 正则候选路径不回归（F-34 确权/剔除逻辑原样保留，28 项 ports 测试通过）
- [x] PID tooltip 展示进程树全部 PID（`pidTooltipLines` 统一三处：
      Applications 表 / Processes 表 / 详情区 PID 行）
- [x] `cargo test`（ports 28 项）+ `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-04 修复完成，测试与构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-04 | ⬜ | 用户反馈录入：端口检测按 PID 推测、PID 展示全 |
| 2026-09-04 | 🟦 | 方案确认（PID 为主 + 正则兜底；tooltip 列全部 PID）；开始实施 |
| 2026-09-04 | ✅ | ports.rs attribution 线程加周期 PID 扫描（merge_tree_owned_ports，含幂等/补记单测）；前端三处 PID tooltip 统一列出进程树全部 PID。验证：cargo test ports（28 通过）+ pnpm build 通过 |
