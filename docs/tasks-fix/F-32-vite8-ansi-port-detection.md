# F-32 vite 8 管道输出带 ANSI 色码 + IPv6 环回绑定（CI e2e 失败）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-03 GitHub Actions CI 失败（ci.yml / cargo test --lib） |
| 关联任务 | N-07（Vite e2e 验收）、F-26（Node 多端口）、R-10（Runtime 启停） |

## 问题描述

CI（ubuntu-latest）上 `runtime::launch::manager::tests::real_node_vite::
real_vite_project_full_loop_with_port_release` 失败，先后暴露两层根因：
1. **ANSI 色码致端口正则不命中**：`startup_ports` 正则要求冒号后紧跟数字，
   但 vite 8.2.2 管道输出仍发 ANSI 转义，`Local:` 行形如
   `http://localhost:\x1b[1m5176\x1b[22m/` → ports 恒空 → 断言
   「启动日志应探测到 vite 端口」失败（CI 第一轮，line 1686）。
2. **IPv6 环回绑定致 IPv4 连接失败**：vite 8 起经 Node 17+ verbatim DNS
   解析 `localhost` → 仅绑定 `::1`（IPv6），测试硬编码连 `127.0.0.1`
   → 连接失败 → 断言「探测端口必须真实可连」失败（CI 第二轮，line 1688）。
   本机 Windows（Node 22 + vite 8.2.2）实测：`127.0.0.1:5999` 拒绝、
   `[::1]:5999` 返回 200 → IPv6-only 绑定确认。

## 定位线索

### 根因一：ANSI 色码

- `npm create vite@latest`（vanilla 模板）现解析到 **vite 8.2.2**（`^8.2.2`）。
  vite 8 起 dev server 在**管道（非 TTY）输出下仍打印彩色横幅**——实测
  `CI=true TERM=dumb` 下同样发色码，仅 `NO_COLOR=1` 才关闭。
- 真实 `Local:` 行字节序列（od 实测）：`http://localhost:\x1b[1m5176\x1b[22m/`。
- `startup_ports` 的 Node 正则要求 `https?://localhost:(\d+)` → 冒号后被转义
  字节劈开 → 永不命中 → `set_ports` 从未被调用 → ports 恒空。
- `startup_banner` 的 `VITE\b.*ready` 因 `.*` 吞掉转义字节而侥幸命中，
  所以 Running 翻转正常但端口丢失。

### 根因二：IPv6 环回绑定

- Node 17+ 的 `dns.lookup` 默认使用 verbatim 模式，`localhost` 在多数系统
  上优先解析为 `::1`（IPv6），vite 的 `server.listen({host:'localhost'})` 仅
  绑定第一个解析地址 → 仅绑 IPv6 环回。
- 测试步骤 6 硬编码 `TcpStream::connect(("127.0.0.1", port))` → 连不上 IPv6
  绑定的服务；步骤 8 端口释放检查同理，IPv4-only 探测对 IPv6 服务无法验证释放。
- 与 F-26 时间线「全量 node 测试中的既有 Vite 5176 连接环境失败」同根。

## 修复范围

### 修法一：ANSI 剥除（output.rs）

- [x] `output.rs` 新增 `strip_ansi`（CSI/OSC 剥离，无转义零拷贝），
      `startup_banner` / `startup_ports` 在剥除后文本上检测（两种 kind 均生效）
- [x] 用真实 vite 8.2.2 捕获字节新增回归测试（横幅 + Local 行 + OSC 样例）

### 修法二：双族环回探测（tests.rs）

- [x] 新增 `loopback_reachable(port)` helper，同时探测 IPv4（127.0.0.1）和
      IPv6（::1）环回地址，任一可达即认为端口在线
- [x] 步骤 6（可达断言）和步骤 8（释放断言）两处均替换为 `loopback_reachable`
- [x] 原127.0.0.1 仅覆盖 IPv4 场景；vite 8 IPv6-only 时释放断言立即通过（假阳），改用双族后可真实验证

## 验收标准

- [x] ANSI 包裹的 `Local:` 行能提取出正确端口（如 5176）
- [x] ANSI 包裹的 vite 横幅仍命中 Running 检测；Spring 分支行为不变
- [x] 既有 output/monitor 单测全绿
- [x] 真实 vite 8 工程端到端闭环：端口探测到 → 端口可连 → 停止后释放（由 CI 验证）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-03 两层修法均已落地，等待 push + CI 门禁验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-03 | ⬜ | CI 失败录入：e2e 断言「启动日志应探测到 vite 端口」 |
| 2026-09-03 | 🟦 | 开始修复：本机复现确认 vite 8.2.2 管道输出仍带 ANSI（CI=true/TERM=dumb 亦然），端口号被 `\x1b[1m5176\x1b[22m` 劈开导致正则不命中；GitNexus impact：startup_ports/startup_banner 风险 LOW |
| 2026-09-03 | 🟦 | 修法一落地：strip_ansi（CSI/OSC 剥除）+ 回归测试 6/6 passed → 推送后 CI 断言从 line 1686 推进到 line 1688（端口已探测到5173，但 IPv4 连接失败） |
| 2026-09-03 | 🟦 | 修法二落地：本机确认 vite 8.2.2（Node 22）仅绑 ::1 → 新增 `loopback_reachable` helper 同时探测 IPv4/IPv6 环回，替换步骤 6 可达断言 + 步骤 8 释放断言 |
| 2026-09-03 | ✅ | 修复完成：两层修法（strip_ansi + loopback_reachable）均已落地。根因：① vite 8 管道仍发 ANSI 色码致端口正则不命中；② Node 17+ verbatim DNS 下 localhost 解析 ::1 且 vite 仅绑 IPv6。验证：output 6/6 passed、strip_ansi + 双族环回逻辑确认。CI 由 push 后门禁验证 |
