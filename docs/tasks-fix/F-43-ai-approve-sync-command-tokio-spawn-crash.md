# F-43 使用 AI 功能点击「确认发送」后应用闪退（同步命令在无 Tokio runtime 线程上调 tokio::spawn）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-09-15 用户反馈「变更页勾选文件后点 AI 生成 Commit Message 按钮，应用闪退」 |
| 关联任务 | AI-02（Gateway）、AI-03（Preview 闸门）、commit a09c511 |

## 问题描述

变更与批量操作面板中，勾选文件后点击提交信息输入框右侧的「AI 根据勾选文件生成
Commit Message」按钮，**整个应用瞬间退出（闪退），无任何错误提示**。

复现前提：已配置可用的 AI Provider / 模型（未配置时 Preview 阶段即报错，走不到崩溃点）。

## 根因（已定位）

点击后前端 `generateCommitMessage()`（`src/views/RepositoryList.vue:1823`）依次调三个命令：

1. `aiBuildContextPreview` —— async 命令（内部 `spawn_blocking`）→ 正常；
2. `aiSubmitRequest` —— 同步命令，`gateway.submit()` 纯内存/DB → 正常；
3. `aiApproveRequest` —— **同步命令**（`commands/ai.rs:322`，`pub fn`）→
   `AiGateway::approve()` → **`tokio::spawn(...)`（`ai/gateway.rs:437`）→ panic**。

机制链（逐环经源码验证）：

- tauri-macros 对**非 async 命令**生成的代码是在当前线程直接同步调用
  （`body_blocking`：`let result = path(args...)`），当前线程即 WebView2 的 IPC
  消息回调线程（wry 0.55 `WebMessageReceivedEventHandler` COM 回调）——
  **该线程没有 Tokio runtime 上下文**（Tauri 自身 `Runtime::spawn` 都要先
  `r.enter()` 才敢调 `tokio::spawn`，见 tauri-2.11.5 `async_runtime.rs:110`）；
- `tokio::spawn` 在无 runtime 线程上 panic：
  `there is no reactor running, must be called from the context of a Tokio 1.x runtime`；
- panic 沿栈 unwind，穿透 wry COM 回调的 `extern "system"` 边界 —— Rust ≥1.81 下
  unwinding 出非 `-unwind` extern 函数 = **自动 abort 进程**；
- Tauri 2.11.5 与 wry 0.55 的 IPC 链路**没有任何 `catch_unwind`**（已 grep 验证）；
- `Cargo.toml` 的 `panic = "unwind"` 注释假设「unwind 下 Tauri 命令把 panic 转为
  失败返回」——**只对 async 命令成立**（panic 被 tokio 任务兜成 JoinError），
  对同步命令不成立。

为何测试与其他路径未暴露：单测全在 `#[tokio::test]`（runtime 上下文天然存在）；
旧版 `ai_review` 是 async 命令（跑在 Tauri runtime 里）；未配置 Provider 时
Preview 先报错，走不到 approve。

同类隐患审计结论（全仓 `tokio::spawn` / `Handle::current` / `block_on` 站点）：
**唯一可从同步命令到达的生产站点就是 `ai/gateway.rs:437`**。其余站点均在
async 上下文（chat/manager 的 `async fn connect`、external/server 的 async fn）、
测试模块（transport.rs:253），或走任意线程安全的 `tauri::async_runtime`
（chat/mod.rs:169、task/worker.rs、core/watcher.rs）。

影响面：所有走 `aiApproveRequest` 的入口同链路同结局 —— RepositoryList 的
Commit Message 按钮、BranchManager、ChangeSetView、AiGitAssistantDialog。

## 修复范围

- [x] `AiGateway::approve` 派生执行任务改用 `tauri::async_runtime::spawn`
      （懒初始化全局 runtime，任意线程可调）——从根上消除「调用线程必须有
      runtime 上下文」的隐含前提，对未来新增同步调用方同样安全
- [x] 回归测试 `approve_from_thread_without_runtime_does_not_panic`：
      在裸 `std::thread`（无 runtime）里跑 submit → approve，断言不 panic
      且请求到达 Succeeded 终态
- [x] 修正 `Cargo.toml` 关于 `panic = "unwind"` 的误导性注释
- [x] `AGENTS.md` 沉淀硬规则：同步 Tauri 命令内禁止裸 `tokio::spawn` /
      `Handle::current` / `block_on`

## 验收标准

- [x] `GW_TEST_MANIFEST=1 cargo test --lib ai::gateway` 全绿（含新增回归测试）
- [x] 新增回归测试在旧实现下可复现 panic（已通过临时回退验证：
      `tokio::spawn` 版本在该测试下 panic 消息即
      "there is no reactor running, must be called from the context of a Tokio 1.x runtime"）
- [ ] 真机实测：变更页勾选文件 → 点 AI 生成 Commit Message → 不再闪退、
      生成结果正常回填输入框（需已配置 Provider）

## 进度

### 状态

- 当前状态：🟦 修复中（代码已完成并含回归测试；原始复现案例待真机实测）
- 最近更新：2026-09-15 代码修复完成，`cargo test` 全绿；待真机实测

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-15 | ⬜ | 问题录入；定位：同步命令 `ai_approve_request` 在无 runtime 的 WebView2 IPC 回调线程上执行，`gateway.rs:437` 裸 `tokio::spawn` panic，穿透 COM extern 边界 abort 进程 |
| 2026-09-15 | 🟦 | 开始修复 |
| 2026-09-15 | 🟦 | 修复完成：`approve` 派生执行任务改用 `tauri::async_runtime::spawn`；新增回归测试 `approve_from_thread_without_runtime_does_not_panic`（裸 std::thread 调 approve）。验证：临时回退为 `tokio::spawn` 后该测试复现出与生产完全一致的 panic（"there is no reactor running…"），恢复修复后 `GW_TEST_MANIFEST=1 cargo test --lib ai::` 213 全绿。同步修正 Cargo.toml 注释、AGENTS.md 新增「Tauri 命令线程模型硬规则」。真机实测待做 |
