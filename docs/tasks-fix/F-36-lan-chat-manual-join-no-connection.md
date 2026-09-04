# F-36 LAN 加密聊天手动输入房间 ID + Secret 无法进入同一房间

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-04 用户反馈（工具箱 LAN 加密聊天第 1 项） |
| 关联任务 | 设计文档 docs/局域网 P2P 加密聊天ao小工具需求与技术设计.md（§15-§17） |

## 问题描述

加入房间时手动输入房间 ID 和加密 Secret（Bootstrap 留空，同子网自动发现），
表面上进房成功，但发送消息后对方看不到——双方实际未建立连接。而点击
「附近房间」列表中发现的房间进入（自动填入房间 ID + Bootstrap 地址）后可以
正常对话。

## 根因（已确诊）

**`ChatManager::start_mdns` 把 `alive` 标志当成了 `stop` 标志传入
`browse_room`，布尔语义颠倒。**

- `discovery/mdns.rs::browse_room` 原签名第三参为 `stop: Arc<AtomicBool>`，
  线程循环条件 `while !stop.load(...)`（true = 停止）。
- `chat/manager.rs::start_mdns` 调用时传的是 `Arc::clone(&self.alive)`——
  房间存活期间 **alive = true** → `!alive = false` → **browse 线程启动后
  一次循环都不执行，立即退出**。
- 后果：进房后的 mDNS 自动发现/自动拨号整条路径形同虚设。advertise 不受影响
  （所以附近房间列表仍能看到房间）；带 Bootstrap 的加入不受影响（直连）。
  唯一失效的就是「手动输入房间 ID + Secret、Bootstrap 留空」这条依赖
  进房后自动组网的路径——与反馈现象完全吻合。
- 之所以一直没暴露：既有集成测试全部 `enable_mdns=false` + 显式 bootstrap，
  mDNS 自动组网路径零覆盖。

诊断过程（同进程对照实验）：advertise daemon 正常响应查询（房间可被解析），
browse daemon 连本地生成的 SearchStarted 事件都未发出 → browse 线程未在运行
→ 逐层隔离（tokio 上下文 / quinn endpoint / 4 daemon 并存均排除）后定位到
stop/alive 语义颠倒。

## 修复内容

1. `discovery/mdns.rs::browse_room`：第三参改为 `alive: Arc<AtomicBool}`
   （true = 继续运行），循环条件 `while alive.load(...)`，与调用方语义一致；
   同时修复 channel 断开（daemon 关闭）时 `Err(_) => continue` 的空转问题——
   Disconnected 直接 break 退出线程。
2. `chat/manager.rs::start_mdns`：调用点不变（传的本来就是 alive，现在语义
   对上了）。
3. 新增回归测试 `lan_chat_mdns_autoconnect_without_bootstrap_or_skip`：
   双节点 enable_mdns=true、无 bootstrap，断言自动组网成功并双向消息互通；
   环境无组播时 skip 并打印原因（项目惯例）。

## 修复范围

- [x] browse_room stop/alive 语义颠倒修复
- [x] browse 线程 Disconnected 空转修复（顺带，同一循环内）
- [x] mDNS 自动组网回归测试（无 bootstrap 路径首次入覆盖）
- [ ] （未做，备选后续）secret / room_id 输入归一化（trim）：本次根因已完全
      解释现象，trim 属独立健壮性改进，不在本任务扩张

## 验收标准

- [x] 手动输入房间 ID + Secret（Bootstrap 留空）后双方可建立连接、成员可见、
      消息互通（回归测试 `lan_chat_mdns_autoconnect_without_bootstrap_or_skip`
      在本机实测通过：自动组网 + 双向消息）
- [x] 点击发现房间进入的原路径不回归（`lan_chat_loopback_two_nodes_exchange_messages`
      等既有测试通过）
- [x] 全部相关 `cargo test` 通过（chat:: 17 项、discovery:: 19 项）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-04 修复完成，全部相关测试通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-04 | ⬜ | 用户反馈录入：手动输入房间 ID + Secret 进房后消息对方不可见；点击发现房间进入正常 |
| 2026-09-04 | 🟦 | 开始修复：定位到两条路径唯一差异为 bootstrap；mDNS 自动组网路径无测试覆盖，先写诊断测试复现 |
| 2026-09-04 | 🟦 | 复现成功（无 bootstrap 25s 未组网）；逐层对照实验排除 tokio/quinn/多 daemon 后，确诊 `start_mdns` 把 `alive` 当 `stop` 传入 `browse_room`，browse 线程启动即退出 |
| 2026-09-04 | ✅ | 修复 browse_room 语义（alive）+ Disconnected 空转；新增无 bootstrap 自动组网回归测试。验证：`cargo test chat::`（17 通过，含新回归真实组网）+ `cargo test discovery::`（19 通过） |
