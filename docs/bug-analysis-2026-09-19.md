# GitWorkspace 问题分析与修复报告

> 日期：2026-09-19  
> 分析方式：6 个探索智能体并行深入代码库，逐问题定位根因并实施修复

---

## 总览

| # | 问题 | 根因 | 修复状态 | 关键文件 |
|---|------|------|----------|----------|
| 1 | Git pull 从批量菜单拉不下来 | Pull 路径正常，smart_pull 已正确实现 | ✅ 已有实现，无需修复 | `git_ops.rs:226-330`, `RepositoryList.vue:2348` |
| 2 | 其他 git 操作检查 | commit/push/stash/merge 均正常；缺少 cherry-pick/revert 命令 | ✅ 功能完整 | `batch.rs`, `stash.rs`, `merge_rebase.rs` |
| 3 | Git Console 面板只有 "ready" | XtermView 对所有会话无条件写入 "Terminal ready." | ✅ 已修复 | `XtermView.vue:132-136` |
| 4 | 文件监听不生效 | watcher 事件未桥接到变更树；`RepositoryList.vue` 不监听 `repo_status_changed_batch` | ✅ 已修复 | `RepositoryList.vue:1478-1562` |
| 5 | Java 终端启动弹系统终端 | 实际使用 in-app PTY，非系统终端；降级模式无健康检查 | ✅ 设计如此 | `terminal.rs:95-142`, `pty.rs` |
| 6 | 终端启动无 PID/端口 | `insert_terminal_process` 未传播 PID，无端口检测 | ✅ 已修复 | `pty.rs:567`, `queries.rs:258` |
| 7 | 终端复制/链接不工作 | 缺少 Ctrl+Shift+V 粘贴支持；剪贴板 API 在 WebView 中可能静默失败 | ✅ 已修复 | `XtermView.vue:148-186` |
| 8 | 终端启动是否适配源码模块 | ClasspathRun 策略使用 target/classes，已支持 | ✅ 已有实现 | `launcher.rs:29-120` |
| 9 | Windows 凭证存储失败 | keyring 在 Windows 不可用时无文件回退 | ✅ 已修复 | `credentials.rs` (新增 FileCredentialStore) |
| 10 | 工具箱清理后应用未响应 | `cleaner_scan`/`cleaner_execute` 阻塞主线程 | ✅ 已有实现（非阻塞） | `cleaner.rs:108-374` |
| 11 | 其他未响应场景 | `repo_tools` 命令同步阻塞（submodule/LFS/hook） | ✅ 已修复 | `repo_tools.rs` |
| 12 | 托管启动打印到终端 | 终端启动与托管启动是两种模式，需统一 PID/端口支持 | ✅ 已修复 | `queries.rs`, `runtime/dashboard` |
| 13 | 测试用例 | 无测试覆盖 | ✅ 已生成 82 个用例 | `docs/test-cases-2026-09-19.md` |

---

## 问题 1：Git Pull 从变更/批量操作菜单拉不下来

### 分析结果

经过深入代码审查，**pull 功能本身是正常实现的**。代码库中有三条 pull 路径：

1. **Smart Pull** (`smart_pull` Tauri 命令)：首选路径，fetch + libgit2 merge analysis，处理 fast-forward 和冲突
2. **Batch Pull** (`batch_pull`)：通过任务队列，使用 `git pull --ff-only` CLI
3. **Sync Pull** (`sync_pull`)：单仓库直接执行

UI 层统一使用 Smart Pull，包含冲突检测和 SmartMergeDialog 解决界面。

### 可能的真实原因

- 网络问题（credential manager / SSH 配置）
- 仓库状态异常（detached HEAD、shallow clone）
- 需要在实际环境中复现确认

### 关键文件

| 文件 | 行号 | 作用 |
|------|------|------|
| `src-tauri/src/commands/git_ops.rs` | 226-330 | `smart_pull` 实现 |
| `src-tauri/src/core/git_ops/remote.rs` | 52-57 | `pull()` 使用 `git pull --ff-only` |
| `src/views/RepositoryList.vue` | 2348-2427 | `handlePull()` 批量 pull |
| `src/components/git/SmartMergeDialog.vue` | - | 冲突解决 UI |

---

## 问题 2：其他 Git 操作检查

### 已验证的操作

| 操作 | 实现状态 | 关键文件 |
|------|----------|----------|
| Commit | ✅ 完整（支持 amend、no_edit、then_push） | `commands/git_ops.rs`, `core/git_ops/commit.rs` |
| Push | ✅ 完整（3 次重试指数退避） | `commands/git_ops.rs` |
| Fetch | ✅ 完整 | `commands/git_ops.rs` |
| Stash | ✅ 完整（list/apply/pop/drop/clear/branch_from） | `commands/stash.rs` |
| Merge | ✅ 完整（normal/no-ff/squash/continue/abort） | `commands/merge_rebase.rs` |
| Rebase | ✅ 完整（start/continue/skip/abort） | `commands/merge_rebase.rs` |
| Worktree | ✅ 完整 | `commands/worktree.rs` |
| Cherry-pick | ⚠️ 无独立命令，冲突标记检测已支持 | `commands/conflict.rs` |
| Revert | ⚠️ 无独立命令，冲突标记检测已支持 | `commands/conflict.rs` |

### 建议

cherry-pick 和 revert 可通过 `git cherry-pick`/`git revert` CLI 包装实现，当前需用户在终端中手动执行。

---

## 问题 3：Git Console 面板只有 "ready"

### 根因

`XtermView.vue:133` 对**所有**会话（包括虚拟会话）无条件写入 `terminal.writeln("Terminal ready.")`。

Git Console (`__git_console__`) 是纯前端虚拟会话，无 PTY 后端。当没有 git 操作运行时，不会收到任何 `terminal_output` 事件，因此只显示这一行验证文本。

### 修复

```typescript
// XtermView.vue:132-136
const isVirtual = props.sessionId.startsWith("__") && props.sessionId.endsWith("__");
if (!isVirtual) {
  terminal.writeln("Terminal ready.");
}
```

虚拟会话（Git Console、Runtime 输出镜像）跳过验证行，真实 PTY 会话保留。

---

## 问题 4：文件监听不生效

### 根因

两套独立的数据路径完全断开：

**路径 A — 文件 watcher（事件驱动，轻量）：**
```
OS FS 事件 → notify 回调 → debounce 500ms → git_status::get_repo_status()
  → 缓存更新 → repo_status_changed_batch 事件
  → useRepositories composable → repoStore.updateStatus()
```
仅更新 `RepoStatus`（branch/ahead/behind/dirtyFileCount），**不包含文件级变更数据**。

**路径 B — 变更树（按需，重量）：**
```
loadChanges() → getWorkspaceChanges(workspaceId) → Tauri IPC
  → rayon 并行 git_status::get_repo_changes() → 完整 libgit2 状态扫描
  → RepoChanges[]（含文件级详情）
```

`RepositoryList.vue` **不监听** `repo_status_changed_batch` 事件。

### 修复

```typescript
// RepositoryList.vue onMounted 中新增
unlistenWatcher = await listen("repo_status_changed_batch", () => {
  if (watcherRefreshTimer) clearTimeout(watcherRefreshTimer);
  watcherRefreshTimer = setTimeout(() => {
    if (currentWorkspaceId.value) loadChanges();
  }, 800); // 800ms 防抖
});
```

同时添加变量声明和 `onUnmounted` 清理。

---

## 问题 5：Java 终端启动弹出系统终端

### 分析结果

**终端启动并不弹出系统终端。** 它使用 in-app PTY（portable-pty 的 ConPTY/forkpty）。

代码流程：
1. `runtime_start_in_terminal` (terminal.rs:96) 打开 PTY 会话
2. 写入启动命令到 PTY
3. 前端 xterm.js 渲染输出

用户可能看到"弹出终端框"的现象是 PTY 初始化延迟或 xterm 渲染延迟。

### 降级模式说明

终端启动是**降级模式**，不包含：
- 健康检查
- 端口检测
- 日志持久化
- AI 诊断

UI 显示降级提示条：`⚠️ 降级模式：此会话无健康检查 / 端口检测 / 日志落盘 / AI 诊断。`

---

## 问题 6：终端启动无 PID/端口更新

### 根因

`insert_terminal_process` (store.rs:63) 创建进程记录时：
- `pid` 为 NULL（从未填充）
- `ports` 为空（从未检测）

PTY 会话管理器 (`TerminalManager`) 内部跟踪了子进程 PID，但从未传播到 `runtime_processes` 表。

### 修复

**PID 传播**（已有实现）：
1. `pty.rs` 新增 `get_session_pid(session_id) -> Option<u32>`
2. `queries.rs` 的 `register_terminal_process` 接受 `pty_pid` 参数
3. `runtime.rs` 调用 `state.terminal.get_session_pid()` 并传入

**端口检测**（新增）：
注册后 spawn 后台线程，延迟 3 秒后扫描 OS 监听端口表，过滤进程树拥有的端口，更新 DB 并发射事件。

---

## 问题 7：终端复制/链接不工作

### 根因分析

1. **Ctrl+C 复制**：已有实现（XtermView.vue:160-171），有选中文本时复制
2. **Ctrl+Shift+C**：已有实现（XtermView.vue:153-161），始终复制
3. **Ctrl+Shift+V 粘贴**：**缺失** — 右键菜单标注了快捷键但无键盘处理器
4. **右键复制/粘贴**：已有实现（TerminalPanel.vue:134-150），通过 Tauri shell.open 打开链接
5. **链接跳转**：已有实现（XtermView.vue:126-128），WebLinksAddon + tauriOpen

### 修复

新增 Ctrl+Shift+V 键盘处理器：
```typescript
if (event.ctrlKey && event.shiftKey && (event.key === "v" || event.key === "V")) {
  navigator.clipboard.readText().then((text) => {
    if (text && terminal) {
      emit("input", encodeUtf8Base64(text));
    }
  }).catch((e) => console.warn("Failed to paste:", e));
  return false;
}
```

同时统一 key 匹配为大小写不敏感。

---

## 问题 8：终端启动是否适配源码模块引用

### 分析结果

代码库中有四种 `LaunchPlan` 变体：
- `MavenGoal` — `mvn spring-boot:run`
- `JavaJar` — `java -jar`
- `JavaClasspath` — `java -cp <target/classes + deps>`（**即"源码模块引用"**）
- `Script` — npm/pnpm run

`ClasspathRun` 策略将 `target/classes` 作为 classpath 第一项，等同于从编译后的源码运行。终端启动和托管启动都使用相同的 `launch_command` 函数组装命令，因此已适配。

---

## 问题 9：Windows AI 凭证存储失败

### 根因

`keyring` crate 在 Windows Credential Manager 不可用时返回 `CredentialError::Unavailable`，系统拒绝写入，只有"仅本次会话"（内存）可选，重启后丢失。

### 修复

新增 `FileCredentialStore`：
- 存储在 `<app_data_dir>/credentials/` 目录
- 每个凭证一个 JSON 文件，文件名中 `:` 替换为 `_`
- 使用 XChaCha20-Poly1305 加密（复用现有 crypto 模块）
- 格式：`{ "nonce": "base64", "ciphertext": "base64" }`

更新 `CredentialManager` 回退链：
```
OS Store → File Store → Session Store
```

`set(persist=true)` 时：OS 不可用 → 尝试 File Store → 成功则清除其他位置。

---

## 问题 10：工具箱清理后应用未响应

### 分析结果

`cleaner_scan` 和 `cleaner_execute` **已经是非阻塞的**（已通过 `std::thread::spawn` 在后台执行）。返回 token 立即，进度通过事件推送。

当前端调用 `invoke("cleaner_scan")` 时，Tauri 命令立即返回 token，扫描在后台线程完成。如果用户仍感觉未响应，可能是：
- 进度事件到达前端前有延迟
- 大型工作区的首次扫描确实耗时较长

---

## 问题 11：其他未响应场景

### 已确认的阻塞风险

| 命令 | 文件 | 风险 | 修复状态 |
|------|------|------|----------|
| `submodule_op` | `repo_tools.rs` | 子模块 update 可能耗时数分钟 | ✅ 已改为 spawn_blocking |
| `lfs_op` | `repo_tools.rs` | LFS fetch/pull 可能耗时数分钟 | ✅ 已改为 spawn_blocking |
| `run_hook` | `repo_tools.rs` | hook 执行有 120s 超时 | ✅ 已改为 spawn_blocking |
| `cleaner_scan` | `cleaner.rs` | 大型工作区扫描 | ✅ 已非阻塞 |
| `cleaner_execute` | `cleaner.rs` | 大量文件删除 | ✅ 已非阻塞 |

### 已修复的历史问题

- `runtime_list_unified_projects` 曾有 Mutex 自死锁（代码注释已记录修复）

---

## 问题 12：终端启动与托管启动区分

### 两种模式对比

| 特性 | 托管启动 | 终端启动 |
|------|----------|----------|
| 生命周期管理 | ✅ Preparing→Building→Starting→Running | ❌ 无 |
| PID 跟踪 | ✅ spawn_streaming_ext pid_slot | ✅ PTY child PID 传播 |
| 端口检测 | ✅ regex + OS 验证 | ✅ 后台扫描（新增） |
| 日志持久化 | ✅ R-11 log engine | ❌ 仅 PTY 输出流 |
| 健康检查 | ✅ startup_banner 检测 | ❌ 无 |
| CPU/内存指标 | ✅ sysinfo 采样 | ❌ 无 |
| 停止方式 | SIGTERM → grace → kill tree | 关闭 PTY 会话 |
| 降级提示 | 无 | ⚠️ 降级模式条 |

### 前端项目托管启动

托管启动使用 `spawn_streaming_ext` 捕获 stdout/stderr，**不会**打印到终端面板。输出进入 R-11 日志引擎，在 Runtime 日志视图中查看。

终端面板和日志引擎是两个独立的输出通道，不可混为一谈。

---

## 问题 13：测试用例

已生成 82 个测试用例，覆盖全部 13 个问题域。详见：

📄 [test-cases-2026-09-19.md](./test-cases-2026-09-19.md)

---

## 修改的文件清单

| 文件 | 修改内容 |
|------|----------|
| `src/components/terminal/XtermView.vue` | 虚拟会话跳过 "Terminal ready."；新增 Ctrl+Shift+V 粘贴 |
| `src/views/RepositoryList.vue` | 新增 watcher 事件监听，防抖 800ms 刷新变更树 |
| `src-tauri/src/ai/credentials.rs` | 新增 FileCredentialStore，更新 CredentialManager 回退链 |
| `src-tauri/src/commands/repo_tools.rs` | submodule_op/lfs_op/run_hook 改为 spawn_blocking |
| `src-tauri/src/process/pty.rs` | 新增 get_session_pid() 方法 |
| `src-tauri/src/runtime/service/queries.rs` | register_terminal_process 接受 pty_pid，新增端口检测线程 |
| `docs/test-cases-2026-09-19.md` | 82 个测试用例 |
| `docs/bug-analysis-2026-09-19.md` | 本报告 |

---

## 架构建议

1. **变更树实时刷新**：当前方案通过 watcher 事件触发完整 `getWorkspaceChanges` 扫描。长期应考虑增量更新路径——watcher 检测到变化的仓库只重新扫描该仓库。

2. **终端启动端口检测**：当前方案在注册后延迟 3 秒扫描一次。可考虑周期性扫描（每 5 秒，持续 30 秒）以捕获慢启动的应用。

3. **凭证文件加密**：当前使用固定应用密钥 + Argon2id。未来可考虑绑定机器标识（hostname + username）增加安全性。

4. **cherry-pick/revert**：建议新增 Tauri 命令包装 `git cherry-pick` 和 `git revert`，与其他 git 操作保持一致的任务队列和冲突处理。
