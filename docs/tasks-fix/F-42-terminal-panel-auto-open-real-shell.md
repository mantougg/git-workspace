# F-42 终端面板不自动打开真终端（默认落在不可关闭的 Git Console tab）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | 🟦 修复中（代码已完成，原始复现案例待人工实测） |
| 来源 | 2026-09-15 用户反馈「3b5618d 这个提交的功能部分没有生效」 |
| 关联任务 | TM-02（终端面板）、TM-04（Git Console 镜像）、TM-07（终端打磨）、commit 3b5618d |

## 问题描述

`3b5618d fix(terminal): 默认打开终端会话 + 优化 ANSI 颜色` 声称「首次打开终端面板时自动打开
一个真正的终端会话」，实测无效：

- **点击 StatusBar 的「终端」按钮**打开面板，面板里只有一个 `Git Console` tab（🔀 图标），
  没有任何真实终端会话，且该 tab 没有关闭按钮（关不掉）；
- 面板内容区一片空白（没有任何 tab 处于 active）；
- 命令行 `终端：切换终端面板` 同样如此。

用户期望：**不论是不是首次打开**，打开终端面板都应自动给出一个真正的终端；默认 shell 优先
`PowerShell 7`（其次 `Windows PowerShell`、`CMD`）；`Git Console` 不应该成为默认展示的 tab。

## 根因（已定位）

4 处缺陷叠加：

1. **自动开会话的逻辑挂在错误的分支上**（`src/stores/terminal.ts` 原 106-119 行）：
   只有 `showPanel()` 里带自动开会话逻辑，且被 `if (!listenersRegistered)` 门控——只在
   「事件监听尚未注册」这一次生效。而 StatusBar 终端按钮（`StatusBar.vue:38`）与命令
   `terminal:toggle`（`commands/registry.ts:217`）走的是 **`togglePanel()`**
   （原 97-104 行），它**完全没有**自动开会话逻辑 → 点按钮打开面板永远不开终端。

2. **`activeTabId` 开局为 null 且无人设置**：`ensureGitConsoleSession()` 只 `unshift`
   会话、不碰 `activeTabId`；`refreshSessions()` 的兜底要求 `activeTabId` 非空才生效。
   于是首次打开时 `activeTabId` 恒为 `null` → `TerminalPanel.vue:353` 的
   `session.sessionId === activeTabId` 对所有会话为假 → 内容区全空。

3. **Git Console 被无条件创建且不可关闭**：`registerEventListeners()` 开局就调
   `ensureGitConsoleSession()`，任何一次打开面板都会挂上这个非真终端 tab；同时
   `closeTab()` 对它有硬早退 + `TerminalTabs.vue` 的关闭按钮 `v-if` 排除 →
   唯一存在的 tab 恰好是关不掉的那个，表现为「默认展示 Git Console 且关不掉」。

4. **默认 shell 选择在前端不可见**：后端 `detect_default_shell()`
   （`src-tauri/src/process/pty.rs:91-99`）顺序已是 `pwsh → powershell → cmd`，
   但 `openSession()` 用 `title: params?.shell ?? "Shell"`——不传 shell 时不体现
   「PowerShell 7」；且 `refreshSessions()` 会用 Rust 的 exe 文件名（`pwsh.exe`）
   覆盖前端标题。

> 注：后端 shell 探测链路本身正确，未改动探测逻辑（`pty.rs:64-70` 候选顺序 +
> `pty.rs:304-315` 的 `find_in_path` 解析）。

## 修复范围

- [x] `togglePanel()` 与 `showPanel()` 统一走 `openPanel()`：注册事件监听（一次性）→
      保证存在一个**存活的真终端会话**（没有就新建，有就聚焦）
- [x] `ensureRealSession()` 幂等：重复调用不重复建会话；已有存活真终端且用户已选中
      有效 tab（真终端 / runtime 输出）时不抢焦点
- [x] 打开面板后 `activeTabId` 不再为 `null`（内容区不再空白）
- [x] Git Console 改为**懒创建**（只在真正收到 `git_op_output` 镜像输出时出现），并**允许关闭**
- [x] 默认 shell 取探测列表第一个（后端顺序即 `pwsh` 优先），tab 标题显示 shell 的 label
      （`PowerShell 7` / `Windows PowerShell` / `Command Prompt`）而非 `Shell` / `pwsh.exe`
- [x] 自身会创建会话的调用方（`terminal:new-shell` 命令、`launchInTerminal`）传
      `showPanel({ autoOpen: false })`，避免一次点击开出两个 shell
- [x] 顺带清理：`listenersRegistered` 与 `listenersReady` 语义重复（前者修改后成为死变量），
      移除前者；`drainPendingOutput` 抽出重复三处的排干逻辑（并改用 `concat` 规避
      `push(...pending)` 的爆栈风险，同 PAF-04）
- [x] 后端补一条契约测试 `detected_shells_preserve_candidate_order` 锁死候选顺序
      （前端默认 shell 依赖它）

## 验收标准

- [ ] 首次点击 StatusBar「终端」按钮，面板自动出现一个 `PowerShell 7`（无 pwsh 时为
      `Windows PowerShell` / `Command Prompt`）tab，且它是当前 tab，内容区可见
- [ ] 关闭面板再打开（非首次），仍有可用真终端：已有存活会话则直接聚焦，不新建重复会话
- [ ] 手动关掉所有真终端后再打开面板，会重新自动开一个
- [ ] 无 git 操作时面板里**不再有** `Git Console` tab
- [ ] 执行一次 git 操作（如 fetch/pull）后 `Git Console` tab 出现并显示镜像输出，且可以关闭
- [ ] `terminal:new-shell` 命令只开出**一个** shell
- [ ] `launchInTerminal`（在终端中启动 runtime）只开出**一个**会话（不额外多一个默认 shell）
- [x] `pnpm build`（vue-tsc + vite）通过

> 前 7 条是 UI 交互行为，需要在真机上点一遍确认。本会话的桌面控制通道
> （computer-use broker）始终 `broker_not_accepting`，无法点按钮实测，故未勾选。

## 进度

### 状态

- 当前状态：🟦 修复中（代码已完成；原始复现案例待人工实测）
- 最近更新：2026-09-15 代码修复完成，`pnpm build` + 新增后端契约测试通过；已产出新 exe

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-15 | ⬜ | 问题录入；定位：自动开会话只在 showPanel 的 `!listenersRegistered` 分支 + togglePanel 无此逻辑 + activeTabId 恒 null + Git Console 无条件创建且不可关闭 |
| 2026-09-15 | 🟦 | 开始修复 |
| 2026-09-15 | 🟦 | 代码修复完成；验证：`pnpm build` 通过、`GW_TEST_MANIFEST=1 cargo test --lib process::pty` 中新增 `detected_shells_preserve_candidate_order` 通过（本机 pwsh 在 `C:\Program Files\PowerShell\7\pwsh.exe`，候选序 pwsh → powershell → cmd）、`pnpm tauri build` 产出 git-workspace.exe 与 GitWorkspace_0.5.1_x64-setup.exe。UI 交互条目待真机实测 |
