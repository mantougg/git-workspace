# F-34 前端多服务启动的 PID/端口归属错误

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] `output.rs`：候选端口语义注释与用例更新（引用型 URL 不再默认可信）
- [ ] `monitor.rs`：新端口 pending → 去抖批量确权 → 移除误收端口并发事件
- [ ] 进程树 PID 枚举工具（kill_tree.rs 或新模块，含 N-07 组兜底）
- [ ] `store.rs` + IPC 类型：持久化监听 PID 列表，`RuntimeProcessInfo.pids`
- [ ] 前端 Applications/Processes 两表格 PID 多值展示 + 端口确权口径一致
- [ ] 多服务 fixture 回归：双监听 + 输出引用后端 URL 的行

## 验收标准

- [ ] `yarn serve` 拉起 8081/8820 两个服务：两端口都展示，PID 列可见全部
      监听进程（根 PID + N 提示）
- [ ] 输出中引用的后端端口（如 `http://localhost:8080` 文本）不出现在
      该应用的端口列表
- [ ] Stop 后端口真实释放（沿用 F-26 E2E 断言口径）
- [ ] 刷新后进程记录的 ports/pids 与实际一致（持久化 round-trip）
- [ ] Windows / Unix 各自确权路径有单测（netstat/lsof 样例纯函数）
- [ ] `cargo test`（manager 相关）+ `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-03 录入用户反馈，附方案分析（候选 + OS 监听表确权）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-03 | ⬜ | 用户反馈录入：多服务 PID 单值 + 端口误收后端；根因定位 startup_ports 正则无归属校验、数据模型无子进程 PID |
