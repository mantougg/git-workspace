# F-34 前端多服务启动的 PID/端口归属错误

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-03 用户反馈（第 2 项） |
| 关联任务 | F-26（多端口收集）、F-32（ANSI 色码探测）、R-10（Runtime 启停）、R-16（端口管理）、N-07（进程组规则） |

## 问题描述

Runtime 总览 Applications 中，前端项目（如 `yarn serve`）一条启动命令会拉起
多个服务（8081、8820 等），但：

1. **PID 只显示一个**——只显示 spawn 的根进程（yarn/npm shim）PID，
   派生出的多个子服务进程 PID 无处可看。
2. **端口收集展示不对，会把后端的端口也收集到**——输出里**引用**的
   后端地址（vite proxy 目标、console.log 打印的 API base URL 等）
   也被当成该前端应用的监听端口。

## 根因分析

- `src-tauri/src/runtime/launch/manager/output.rs::startup_ports`：
  Node 分支用正则
  `https?://(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]):(\d+)`
  匹配**整行输出中的任何 localhost URL**——F-26 让它收集全部端口，但正则
  天然无法区分「本进程树监听的端口」与「日志里引用的端口」。
- `src-tauri/src/runtime/launch/manager/monitor.rs::spawn_monitor`：
  回调把 `startup_ports` 结果直接 `store::set_ports` 持久化并发
  `RuntimeEvent::Ports`，无归属校验。
- `RuntimeProcessInfo.pid`（`runtime/launch/store.rs` 行结构）只存 root
  spawn PID；数据模型没有进程树/监听进程 PID 的字段。

## 方案（选定：输出正则出候选 + OS 监听表确权）

核心思路：正则继续当**候选来源**（保留就绪信号语义，F-26 行为不回退），
新增**归属确权**步骤，用 OS 监听表把「谁真的在监听」钉死：

1. **进程树枚举**：从本次启动的 root PID 出发，用 sysinfo 枚举进程树
   PID 集合（参照 `process/kill_tree.rs` 现有用法；注意 N-07「父死孙活」
   规则——Unix 上 launch 已 `process_group(0)`，树枚举 + 组 ID 双口径兜底）。
2. **端口确权**：对每个候选端口调 `process/port.rs::detect_port_occupier`
   拿监听 PID；**监听 PID ∈ 进程树**才确认为应用端口，否则（如后端 8080
   被前端日志引用）从 `ports_seen` 移除并再次发 `Ports` 事件更正前端。
3. **复核时机与限频**：
   - 新端口出现时先记 `pending`，去抖批量复核（间隔 ≥2s），避免 dev server
     端口晚监听被误删，也避免逐行 spawn netstat；
   - `Command::new("netstat")` 每次一个子进程，Windows 不得高频调用
     （AGENTS.md 平台规范）。
4. **PID 展示**：确权时顺带记录「端口 → 监听 PID」映射，持久化进
   `runtime_processes`（新列或 JSON 字段），`RuntimeProcessInfo` 增加
   `pids: number[]`（树内监听 PID 列表，camelCase serde 已有先例）。
   前端 Applications PID 列显示 `根PID (+N)`，tooltip 列出全部监听 PID 与
   对应端口；应用详情的 PID 行同步多值展示。
5. **前端同步**：`src/types/runtime.ts`、Applications 列
   （`RuntimeDashboard.vue` configColumns PID/端口列 + app-detail-inline）、
   Processes 表格消费同一 `pids` 口径，保持两个表格一致（F-26 验收口径延续）。

### 备选（未采纳）

- 纯前端过滤（如剔除与配置无关的端口）：没有真值来源，规则会越补越脆。
- 定时全量扫监听表反查进程树：信息更全但轮询成本高、时序复杂；
  事件驱动的候选+确权已覆盖需求。

## 修复范围

- [x] `output.rs`：候选端口语义注释与用例更新（引用型 URL 不再默认可信）
- [x] `monitor.rs`：新端口交给 PortAttribution 线程（去抖批量确权）
- [x] 进程树 PID 枚举工具（`process::collect_tree_pids` + 纯函数核心供单测）
- [x] `manager/ports.rs`：attribution 线程（候选→确权→剔除→落库→事件更正）
- [x] `store.rs` + `schema V22`：持久化 `port_pids_json` 列 + `set_port_attribution`
- [x] `process/port.rs`：`ListeningPort` 结构 + 批量 `detect_listening_ports` + 纯函数解析器
- [x] `RuntimeProcessInfo`：`pids` / `port_pids` 字段 + `row_to_info` 派生逻辑
- [x] 前端 `types/runtime.ts`：`pids: number[]` / `portPids: Record<string, number>`
- [x] Applications / Processes 两表格 PID 列多值展示（根 PID + N + tooltip）
- [x] 应用详情 PID 行同步展示 attribution
- [x] IPC golden snapshot 更新（JSON + TS 类型字段对齐）
- [x] 多服务 fixture 回归：attribution 纯函数单元测试（确认/拒绝/重试/兜底）

## 验收标准

- [x] `yarn serve` 拉起多个服务：已确认端口在 portPids 中展示根 PID + 树内
      监听进程数（前端 PID 列 `根PID (+N)`，tooltip 列出端口→PID 映射）
- [x] 输出中引用的后端端口：OS 监听表验证该端口被进程树外 PID 监听时，
      从 `ports` 口径剔除（attribution 纯函数 `Reject` 路径已单测覆盖）
- [x] Stop 后端口真实释放（attribution.stop() + flush_on_stop 落库；
      沿用 F-26 E2E 断言口径）
- [x] 刷新后进程记录的 ports / portPids / pids 与 DB 一致
      （`set_port_attribution` 整体覆盖写 + `row_to_info` round-trip）
- [x] Windows / Unix 确权路径有纯函数单测
      （`parse_netstat_listeners` / `parse_lsof_listeners` + `classify_candidate`）
- [x] `cargo test`（778 passed / 0 failed，pre-existing node::workspace
      verbatim 路径问题已排除）+ `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-03 修复完成，测试通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-03 | ⬜ | 用户反馈录入：多服务 PID 单值 + 端口误收后端；根因定位 startup_ports 正则无归属校验、数据模型无子进程 PID |
| 2026-09-03 | 🟦 | 开始修复：实现 collect_tree_pids + ports.rs attribution 线程 + V22 迁移 |
| 2026-09-03 | ✅ | 修复完成：候选人端口经 OS 监听表确权（树外剔除、树内记录 pid 映射）；DB 新增 port_pids_json 列；前端 PID 列展示根 PID (+N) + tooltip 端口→PID 映射；Applications / Processes 两表口径一致。验证：cargo test 778 passed / 0 failed + pnpm build 通过；golden snapshot 已更新 |
