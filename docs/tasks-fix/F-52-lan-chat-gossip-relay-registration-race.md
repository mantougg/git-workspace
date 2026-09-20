# F-52 LAN 聊天 Gossip 中继偶发丢消息（CI 上 `lan_chat_gossip_relay_via_middle_node` 超时）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-21 GitHub Actions CI（ubuntu-latest，`cargo test --lib`）失败：`chat::manager::tests::lan_chat_gossip_relay_via_middle_node` panicked at `src/chat/manager.rs:1258` — `c should receive relayed message within 10s: Elapsed(())` |
| 关联任务 | F-36（LAN 聊天手动加入组网）、B-07/B-08 记录的负载抖动测试治理 |

## 问题描述

CI 全量 `cargo test --lib`（980 passed / 1 failed / 3 ignored，100.97s）中唯一失败项：

```
thread 'chat::manager::tests::lan_chat_gossip_relay_via_middle_node' panicked at
src/chat/manager.rs:1258:14:
c should receive relayed message within 10s: Elapsed(())
```

A—B—C 链式组网用例：A 发的消息经 B 中继应到达 C，10s 内未到。

本机单跑该用例 60/60 全过（`cargo test --lib chat::manager::tests::lan_chat_gossip_relay_via_middle_node`），
2 CPU / 4 test-threads 全量套件亦复现不出——只有 CI（4 vCPU + 984 个测试并发抢占）踩中。

## 根因（已定位并验证）

**入站方把 peer 登记进 `peers` 表的时机晚于「回握手帧」，形成中继丢包窗口。**

原 `handle_incoming` 的顺序是：

```
accept_bi → 读握手 → validate → 写握手回复 → 写 Peer Exchange → finish_registration（登记 peer）
```

而拨出方 `connect_inner` 的顺序是：

```
open_bi → 写握手 → 读回复 → finish_registration（登记 peer）→ join() 返回
```

于是**拨出方的 `join()` 可以在接收方还没把它登记进 `peers` 表之前就返回**。
用例的拓扑就绪门只断言了 `b.connected_count() >= 1 && c.connected_count() >= 1`
（各自视角、非对称），完全没校验中继跳 B 侧是否已落表 C。

竞态成立时的时序：

1. C `join()`（bootstrap=B）读到 B 的握手回复即返回；此刻 B 还在写 Peer Exchange / 尚未 `register_peer(C)`。
2. 用例门条件已满足（B 有 A、C 有 B）→ 立即 `a.send_message("relay me")`。
3. A → B 送达；B 的 dispatcher 执行 `handle_gossip_frame` 尾部转发
   `broadcast_except(&fwd, Some(from_peer))`，而 B 的 `peers` 表里只有 A、没有 C
   → **转发目标为空，消息静默丢失**，且 gossip 无重传，C 永远收不到。

验证方式：在 `handle_incoming` 的 `finish_registration` 前临时插入 600ms sleep
（模拟 CI 调度饥饿），修复前用例必失败（`Elapsed(())`）；修复后同样注入 800ms
仍稳定通过——证明窗口已被关闭而非单纯被时序掩盖。

## 修复范围

- [x] `chat/manager.rs`：`finish_registration` 新增 `post_register_frames` 参数——
      **先 `register_peer` 落表，再回握手 + Peer Exchange**，把「对端 `join()`
      返回」与「本节点 peers 表已含对端」的先后关系倒过来。入站方把原本直接
      `write_frame` 的两帧改为随参数传入、登记后写出。
- [x] `chat/manager.rs`：`spawn_writer` 改为接收 `(rx, send)` 并推迟到控制帧
      写完之后再 spawn——避免 writer 任务提前启动后，排队帧插到握手回复前面。
      出站方传空 `Vec`（其握手帧已在拨号时直接写出，行为不变）。
- [x] `chat/manager.rs`：`handle_incoming` 移除不再需要的 `mut send`；
      `finish_registration` 加 `#[allow(clippy::too_many_arguments)]`
      （同 `task/worker.rs`、`runtime/build/pipeline/mod.rs` 既有做法）。
- [x] 测试侧守卫：拓扑就绪门改为
      `a.connected_count() >= 1 && b.connected_count() >= 2 && c.connected_count() >= 1`
      （显式要求中继跳 B 侧同时持有 A 与 C），失败时打印三方 peers 数；
      消息超时断言改为带 peers/members 计数的 `panic!`，便于下次 CI 抖动定位。

不在本次范围：gossip 本身仍是无重传的尽力而为转发（用户看到成员列表收敛后再发消息，
真实场景不受影响）；不做周期重广播——那会改变 §21-§23 的协议语义。

## 影响分析（GitNexus 纪律）

改动符号全部为 `chat/manager.rs` 内的私有函数，无跨模块调用方：

| 符号 | 可见性 | 直接调用方 | 风险 |
|---|---|---|---|
| `finish_registration` | private | `connect_inner` / `handle_incoming`（同文件） | LOW |
| `spawn_writer` | private | `finish_registration` | LOW |
| `handle_incoming` | private | `spawn_accept_loop`（同文件） | LOW |
| `connect_inner` | private | `connect`（同文件） | LOW |

公共入口（`create` / `join` / `connect` / `leave` / `send_message` /
`connected_count` / `member_count` / `snapshot`）签名与语义均未变；
对外唯一可观测差异是入站 peer 落表时刻略提前。
受影响执行流：LAN 聊天连接建立 + gossip 中继，无 HIGH/CRITICAL 风险。

（本会话 GitNexus MCP 工具未连接、索引 stale，故按上表以调用点 grep 逐字节审计替代
`impact()`；同 B-01 的记录方式。）

## 验收标准

- [x] 竞态注入验证：`handle_incoming` 注入 600ms / 800ms 延迟后用例仍通过
- [x] `cargo test --lib chat::` 22/22 全绿（连跑 5 次）
- [x] 全量 `cargo test --lib` 无新增失败（仅余既有墙钟基准抖动：
      `runtime_benchmark_smoke` / `revision_diff_cache_hit_returns_identical_result`，
      单独复跑通过，与本改动无关）
- [x] `cargo clippy --lib` 在 `chat/manager.rs` 零告警（基线为 0）
- [x] `rustfmt`：本次新增代码零格式漂移（文件整体漂移行数由基线 303 → 293，
      全部为仓库既有的 rustfmt 1.9 工具链漂移）

## 进度

### 状态

- 当前状态：🟦 修复中（代码 + 单测完成，竞态注入已验证；待 CI 复跑确认）
- 最近更新：2026-09-21 修复完成，竞态注入 800ms 仍通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-21 | ⬜ | 问题录入：CI `lan_chat_gossip_relay_via_middle_node` 10s 超时；本机单跑 60/60 与 2/4 CPU 全量套件均复现不出 |
| 2026-09-21 | 🟦 | 定位根因：入站方 `register_peer` 晚于回握手帧，拨出方 `join()` 可先返回；用例拓扑门非对称（未校验中继跳 B 侧已落表 C），导致 gossip 转发目标为空、消息静默丢失。以「`handle_incoming` 注入 600ms sleep」复现失败证实 |
| 2026-09-21 | 🟦 | 修复完成：`finish_registration` 增加 `post_register_frames`，改为先登记后回握手 + Peer Exchange；`spawn_writer` 改收 `(rx, send)` 并推迟 spawn；测试侧拓扑门改为显式校验中继两跳 + 超时断言带 peers/members 诊断。注入 800ms 延迟仍通过；`chat::` 22/22 连跑 5 次全绿；clippy 该文件 0 告警；新增代码零 rustfmt 漂移。待 CI 复跑确认 |
