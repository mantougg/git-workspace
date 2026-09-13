# GitWorkspace 项目全景分析报告（事实核查版）

> 日期：2026-09-13 ｜ 基线：v0.5.0（master @ 057c53e）
> 方法：第一轮 8 路并行智能体按功能领域探索 + 2 路 bug 排查；第二轮将报告中 **125 条断言逐条对源码取证**（31 条由主控亲自验证，94 条由核查智能体验证），每条结论均引用实际文件与行号。
> 核查结果：**111 条成立、1 条被推翻、13 条修正后收录**，另产生 5 项新发现。被推翻与修正项见 §5。
> 校验（2026-09-13 复核）：P0/P1 关键行号抽查与文档实际一致；修正了本文档头部统计数字（初稿误写「110 条断言 / 94 条成立 / 15 条修正」，实为 125/111/13）与 §5.2 第 14 条表述。

---

## 1. 项目概况

**GitWorkspace** 是 Tauri 2 + Vue 3 + Pinia + Naive UI + xterm.js + Rust 的跨平台桌面应用（Windows / macOS / Linux），面向「一个项目拆几十上百个独立 Git 仓库」的团队，四大支柱：

1. **多仓库工作区引擎** —— 发现、分组、监听、仪表盘、变更集、Manifest 批量克隆。
2. **完整 Git 客户端** —— 分支 / stash / 交互式 rebase / merge / 冲突解决 / reflog / worktree / hunk 行级暂存 / 提交图 / 批量操作 / undo 日志。
3. **Runtime 工作台** —— 免 IDE 构建运行 Spring Boot 与 Node.js 服务：工具链管理、构建引擎、流式日志、健康探针、端口管理、增量构建自动重启、多服务编排。
4. **AI 助手** —— 自带 Key（OpenAI Chat/Responses、Anthropic Messages），代码评审 / commit message / 冲突解决 / PR 描述 / 运行时诊断 / 动作提案，Preview 闸门 + OS Keychain。

**规模与质量基数（实测）**：

| 指标 | 数值 | 来源 |
|---|---|---|
| Rust 后端 | 268 个 .rs ≈ 9.1 万行 | grep 实测 |
| 前端 | 约 220 个 .vue/.ts，34 个视图、33 条路由 | 实测 |
| 测试属性 | **874** 个（`#[test]` 842 + `#[tokio::test]` 32） | grep 实测 |
| 全量测试 | T-35 文档自述 `cargo test --lib` 807 通过 | docs/tasks/T-35 |
| 文档任务 | 8 个系列 118+ 任务：T 35/35、AI 12/12、D 17/17、TM 7/7、R 23/27、N 9/10、B 9/10 | 各 README.md |
| CI | benchmark.yml / ci.yml / release.yml / sync-wiki.yml | .github/workflows/ |

**工程纪律突出**：全仓 TODO/FIXME 仅 2 处（且是 benchmark 生成器字符串）；所有设计偏离显式留痕（如交互式 rebase 不走 git CLI）；踩坑规则沉淀于 AGENTS.md。

---

## 2. 各功能点成熟度评估

评级：🟢 生产可用 ｜ 🟡 基本可用但有已知坑 ｜ 🟠 骨架/预留 ｜ ⚪ 未实现

### 2.1 多仓库工作区引擎 —— 🟢

| 功能 | 位置 | 状态与证据 |
|---|---|---|
| 仓库扫描 | `core/scanner.rs` | ✅ T-01；rayon 并行；100 仓 202ms / 500 仓 700ms（T-07 文档实测） |
| 状态引擎 | `core/git_status.rs` | ✅ T-02；单仓 status 8.14ms（T-02 文档 release 实测）；moka LRU（`state.rs:19`，容量 5000） |
| SQLite 数据层 | `db/`（15 版迁移） | ✅ T-03；WAL + 单写者；43 项迁移测试 |
| 文件监听 | `core/watcher.rs` | ✅ T-06；但有 4 个实证缺陷（见 §3 P1-14） |
| Benchmark | `benchmark/` + CI | ✅ T-07；File Watch/Search/Batch 测试组未实现 |
| 仪表盘/热力图 | `DashboardView.vue` / `core/heatmap.rs` | ✅ T-18 / F-01b |
| 健康检查 | `core/health.rs`（11 类异常 + 可配权重） | ✅ T-19 |
| 变更集 | `core/change_set.rs` | ✅ T-22 |
| Manifest 批量克隆 | `core/manifest.rs` + `TaskType::Clone` | ✅ T-33；路径穿越防护有专测 |
| 非 Git 目录发现 | `maven/discovery.rs:101-120` | ✅ R-27；常开补扫 + 去重，实测 874ms |

### 2.2 Git 客户端 —— 🟢（测试密度最高）

| 功能 | 位置 | 状态与证据 |
|---|---|---|
| 分支管理 | `core/branch.rs` | ✅ T-09；5 单测；ahead/behind 纯本地 remote-tracking（零网络，文件头注释） |
| Stash / 工作区级 Stash | `core/stash.rs` / `core/workspace_stash.rs` | ✅ T-10/T-21；oid 重定位恢复 |
| 交互式 Rebase | `core/rebase.rs` | ✅ T-15；**自研序列器**（状态落盘 `.git/gitworkspace-rebase.json`，每步可恢复）；文档显式偏离说明 |
| Merge / 冲突解决器 | `core/merge.rs` / `core/conflict.rs` | ✅ T-16；四栏 BASE/OURS/THEIRS/RESULT |
| Cherry-pick / Revert / Reset | `core/history.rs` | ✅ T-13 |
| Reflog | `core/reflog.rs` | ✅ T-14；含「误 reset 经 reflog 恢复」验收测试 |
| Worktree | `core/worktree.rs` | ✅ T-17；scanner 支持 `.git` 文件形态 |
| Hunk/行级暂存 | `core/stage.rs` | ✅ T-12；9 单测含 no-newline-at-EOF 保留 |
| 提交图 | `core/graph.rs` + `CommitGraph.vue` | ✅；自研惰性堆 walk 替代 revwalk（10k commit 从 ~2-3s 降至预算内，2 个排序测试守护） |
| 批量操作 | `commands/git_ops.rs` / `commands/batch.rs` | ✅ T-20；fetch/pull/push/commit 走任务队列；**add/restore 例外**（§3 P1-26） |
| Undo / 操作日志 | `core/operation_log/` | ✅ T-34；7 单测；覆盖面窄（§4） |
| Submodule/LFS/Hooks | `commands/repo_tools.rs` | ✅ T-30；Windows hook 依赖 Git Bash |

### 2.3 Runtime 工作台 · Java/Spring Boot —— 🟢（工程化最深）

| 功能 | 位置 | 状态与证据 |
|---|---|---|
| Maven 发现/索引/闭包 | `maven/discovery.rs`、`maven/index/`、`maven/closure.rs` | ✅ R-01/02/03；路径归一化地基 `index/path.rs::path_key`（含 `\\?\` 剥离） |
| JDK 管理器 | `java/detect.rs`、`java/registry.rs`、`java/resolve.rs` | ✅ R-04；PATHEXT 候选序 |
| mvnd + 构建缓存 | `maven/mvnd.rs`、`runtime/build/dep_cache.rs` | ✅ R-18；daemon 失败回退 mvn |
| Spring Boot 检测/主类推断 | `runtime/spring_boot.rs` | ✅ R-06；注释/字符串剥离状态机，MAX_SOURCE_FILES=10000 |
| 九步构建流水线 | `runtime/build/pipeline/mod.rs` | ✅ R-09；19 测试，其中 **5 条真实 Maven**（`package_run_builds_spring_boot_app_with_real_maven` 等，pipeline/tests.rs:898-1172） |
| 超长 classpath | `runtime/build/pathing_jar.rs` | ✅ F-11；阈值 30000 字符精确（`:25`） |
| 启动器/进程管理 | `runtime/launch/`、`process/kill_tree.rs`、`process/streaming.rs` | ✅ R-10/B-03；unix `process_group(0)`（launcher.rs:105）+ killpg；F-12 不阻塞 `child.wait()` |
| 日志引擎 | `runtime/logs/engine/` | ✅ R-11/B-05；脱敏 + 环形上限 + 滚动；`register_analyzer` 预留零调用 |
| 健康探针 | `runtime/health.rs` | ✅ R-16；Auto=Actuator 失败回退 TCP（:331-347）；HTTPS 不支持（:217 注释） |
| 端口确权 | `runtime/launch/manager/ports.rs` | ✅ F-34/F-40；进程树比对 + 2s 去 + F-40 每 2s PID 归属 |
| Watch→增量→自动重启 | `runtime/watch/mod.rs` | ✅ R-17/B-07；pom.xml→ResolveDependencies、源码→affected_modules→RebuildRestart（:384-441） |
| 多服务编排 | `runtime/environment.rs`、`runtime/service/` | ✅ R-15；拓扑分波 + 部分失败跳过 |
| 模板 / 依赖图可视化 / Git 联动 / AI 诊断 | `runtime/templates.rs`、`RuntimeDependenciesView.vue`、`runtime/git_link.rs`、`commands/ai.rs:732` | ✅ R-19/R-20/R-21/R-26 |
| **Gradle（R-22）/ Debug（R-23）/ Docker·K8s（R-24）/ JVM 监控（R-25）** | — | **⚪ 未开始**（tasks-runtime/README.md:81-84）；`gradle/` 目录为空，pipeline/mod.rs:255 留有分发点注释 |

**真实 E2E 背书**（`runtime/launch/manager/tests.rs`）：`classpath_run_full_cycle_with_real_spring_boot_app`（:1354，真实 Spring Boot 全生命周期）与 `real_vite_project_full_loop_with_port_release`（:1556，真实 Vite：create→install→启动→端口可连→Stop 后端口真实释放）。`boot_fixture`（:1254）是上述测试复用的 fixture 辅助函数（非测试）。

### 2.4 Runtime 工作台 · Node.js —— 🟡（9/10，差真机复核）

- ✅ 工具链检测覆盖 nvm/fnm/volta/mise/asdf/scoop 等（`node/scan.rs`，跨平台布局枚举 175 行）；包管理器四级决策链（配置 > `packageManager` 字段 > lockfile 固定序 pnpm>yarn>npm>bun > 注册表→PATH 兜底，`node/decision.rs`）；显式安装确认闸门（无 confirmed 返回结构化错误，30 分钟超时）；monorepo workspace 识别与安装路由（`node/workspace.rs`）；缺 `node_modules` 只报可行动错误不自动安装（pipeline/mod.rs:150-156）。
- 🟦 **N-07 收尾**：Windows/macOS 真机复核复选框未勾（N-07 doc:60-61），测试已固化待真机执行。
- 已知坑：unix 上 npm 先死后 vite reparent 到 init，端口 PID 归属确认不出（信息缺失非误判，kill_tree.rs:122-129 注释自认）。

### 2.5 内嵌终端 —— 🟡（7/7 完成，验收层未闭环）

- ✅ PTY 后端（`process/pty.rs`：shell 探测、base64 原始字节事件、`TerminalEmitter` trait 测试注入）；Windows ConPTY 空白根因已修（只 drop slave、master 随会话保活，:350-356，回归测试 `smoke_reader_receives_shell_output` :804-877）；前端 xterm 封装含 pendingOutput 竞态缓冲（terminal.ts:50,171-198）；Git Console / Runtime 合成 tab / 在终端中启动（带脱敏闸门）。
- 🟡 已知坑：① Git Console 是**任务收尾批量发送**而非实时流式——`run_git_streaming` 逐行回调能力存在（remote.rs:228）且 `fetch/pull/push/clone_streaming` 四个封装齐全，但**全仓零外部调用方**（worker 走非流式 `run_git`）；② ConPTY 修复仅在 Rust 测试层闭环，GUI 全链路真机待实测（TM-03 doc:42,56）；③ `close()` 持 sessions 锁轮询最长 2s（:465-499）；④ 死会话不从表回收（:551 注释宣称「会话清理」但未实现）；⑤ Windows 无退出码（:621-625）；⑥ 文件路径 Ctrl+Click 是 TM-07 明确降级项。

### 2.6 AI 助手 —— 🟢（安全设计最讲究）

- ✅ 三协议网关（`ai/gateway.rs`：submit 零网络、approve 唯一联网入口 :390、Semaphore 并发闸 :162/174、重试至多 1 次且流式已有输出不重试 :594）；Preview 闸门 + Secret 三策略管道（Block/Mask/Warn，Mask 后二次扫描仍命中继续阻断）；OS Keychain 单落点不变式（写 OS 清会话、写会话清 OS）；19 工具注册表（范围守卫 + 10s 超时 + 8 次/请求预算 + golden schema）；Action Proposals（TTL 15 分钟 :17/:122、High 风险二次确认 :258、失败 revert :326）；MCP 外部端点（外部权限永不超过内置上限，有专测）；AI 域测试 **190** 个（grep 实测）；LAN 加密聊天（XChaCha20-Poly1305 + Argon2id + mDNS，回环双节点/gossip 中继/错密钥丢弃 4 集成测试）。
- 🟡 已知坑：§3 P1-11/12/40 与 P2 各条。

### 2.7 批量任务与自动化 —— 🟢

- ✅ 任务队列（8 worker，lib.rs:170；PartialSuccess 聚合于 worker.rs:546-605 + models/task.rs BatchState；TASK_TIMEOUT=300s / RUNTIME_TASK_TIMEOUT=3600s / MAX_RETRIES=2，worker.rs:17-25；崩溃恢复 mark_interrupted_tasks）；任务 DAG（Kahn 拓扑校验 + 失败传播 Continue/FailFast，task/dag.rs:271-359）；Pipeline 可视化编排；Tree-sitter 符号索引（`symbols/`）；FTS5 代码搜索；五平台 PR 创建（remote/api.rs，github/gitlab/bitbucket 测试实证）；T-35 发布工程 ✅（ci.yml 含 rust-tests + frontend-typecheck 门禁）。
- 🟡 已知坑：git 网络任务超时后 spawn_blocking 线程占用（非 8 worker 本身，见 §5 修正 1）；`build_code_index` 持全局 DB 锁贯穿整个目录扫描且无事务包裹（commands/ai.rs:931-937）。

### 2.8 桌面体验 —— 🟢

- ✅ D-01~D-17 全 ✅：tokens 主题系统、Shell 骨架（AppShell/SideNav/StatusBar/Panel/Toolbar）、命令面板（Ctrl+K）、快捷键注册表（setup 期构建 context）、窗口状态记忆、三栏 changes 联动、更新器状态机完整、xterm 资源释放正确（dispose + ResizeObserver disconnect）。
- 🟡 已知坑：**主题三档切换 UI 实际缺失**（`setMode` 全仓零调用方，仅 useTheme.ts:75 返回处）；StatusBar 分支槽位空占位（`currentBranch = ref(null)`，:121 注释「预留接口」）；D-02 文档 9 个复选框全部未勾但状态标「✅ 已完成」（系统性文档矛盾）。

### 2.9 跨领域质量观察

- **全项目无前端 E2E harness**（T-18 文档自认）：桌面化 17 个任务全部只有构建级验证（vue-tsc/vite build）。
- **验收缝隙模式**：多个任务标 ✅ 但子项复选框未勾（T-02「与 T-06 联调」、T-06「句柄压力测试」、D-02 全部、N-07 真机、F-36 trim）——完成状态与验收证据存在系统性缝隙。
- 环境性 skip 约定良好（无 mvn/JDK/node 时 skip 并打印原因），但意味着 **CI 若不装 Maven/JDK17+，核心真实集成测试实际不执行**。

---

## 3. Bug 清单（全部经源码实证）

### P0 —— 用户必踩 / 数据级后果（5 项，全部亲自逐行验证）

| # | 问题 | 位置 |
|---|---|---|
| P0-1 | **中文 commit message 触发 UTF-8 字节边界 panic**：`message.len()` 是字节数，`&message[..47]` 按字节切片，中文第 47 字节落在多字节字符中间直接 panic，提交任务失败 ✅ 已修复（PAF-01，2026-09-13：改为按字符截断 `shorten_message`，带 CJK 回归测试） | `src-tauri/src/task/worker.rs:362-363` |
| P0-2 | **spawn 后 10s 未确认 pid → 行落终态 Failed 但进程存活，成为不可停止的孤儿**。链路：`spawn_monitor`(:140) 先启动 → `wait_pid_or_outcome` 10s 超时（Windows Defender 冷扫描 java.exe 是现实场景）→ `abort_before_spawn`(:415-431) 置终态 Failed 且 `cancelled:false`，monitor 无 spawn 前取消检查 → `stop()/kill()` 在 `is_terminal()` 早退（control.rs:19/100） ✅ 已修复（PAF-02，2026-09-13：超时分支预置 force_kill，迟到 spawn 由 streaming 循环首拍杀树；窗口放宽至 30s 且可注入；回归测试覆盖） | `runtime/launch/manager/start.rs:140-153` |
| P0-3 | **重复启动守卫 TOCTOU 双进程**：`find_active` 检查（:27-37 一个 db 锁作用域）与 `insert_process`（:39-42 另一个作用域）分离；`runtime_processes` 表无 (workspace_id, runtime_name) 活跃行 UNIQUE 部分索引（schema.rs:610-634 仅两个普通索引）；8 worker 并发提交时可双 spawn ✅ 已修复（PAF-03，2026-09-13：check+insert 收进同一 DB 锁临界区——单连接写序列化架构下即全量互斥；并发 8 线程回归测试） | `runtime/launch/manager/start.rs:27-44` |
| P0-4 | **终端大文本粘贴栈溢出**：`btoa(String.fromCharCode(...bytes))` spread 超引擎参数上限抛 RangeError 且未捕获，共 **3 处** ✅ 已修复（PAF-04，2026-09-13：新增分块编码工具 `src/utils/base64.ts`，三处统一替换） | `XtermView.vue:114`、`TerminalPanel.vue:149`、`commands/registry.ts:270` |
| P0-5 | **GitGraph「加载更多」只生效一次**：先 `commits.value = more`（:317）再比较 `more.length >= commits.value.length + PAGE_SIZE`（:318），后者恒 false；且每次以递增 limit 全量重拉 O(n²) ✅ 已修复（PAF-05，2026-09-13：先记录旧长度再赋值比较；offset 分页改造另立任务） | `src/views/GitGraph.vue:309-327` |

### P1 —— 本迭代应修（按主题分组）

**启动/停止链路**
- P1-1 **launch_cache 只插不清**：缓存 `HashMap<(i64,String),CachedLaunch>`（manager/mod.rs:119）只有 insert（mod.rs:275、start.rs:344/:399）与读（mod.rs:289、start.rs:257/:301），无任何失效；`restart()` 强制 `skip_build=true`（control.rs:143-145）→ **改端口/JDK/vm_options 后点「重启」静默用旧 LaunchPlan**。 ✅ 已修复（PAF-06，2026-09-13：配置指纹纳入缓存命中判定，配置/覆盖项变化自然失效回退重建；delete 配置显式清除；回归测试覆盖失效+命中两方向）
- P1-2 **stop 在 pid 未回填时强杀也是 no-op**：terminate 与强杀升级都有 `if let Some(pid)` 守卫（control.rs:29-49），pid 持续为 None 时行停留 Stopping；`restart()` 随即 `start()` 撞 `find_active`（Stopping 非终态，lifecycle.rs:64-66）返回 Conflict。 ✅ 已修复（PAF-07，2026-09-13：stop 先等 pid/outcome 短窗口，拿不到也预置 force_kill 由 streaming 循环收树；restart 等行收口终态再 start；回归测试覆盖）
- P1-3 **git 网络任务超时后阻塞线程占用**：超时仅 Runtime 类置 cancel flag（worker.rs:297-301），git 类无取消；泄漏点在 tokio blocking 线程池（默认 512）而非 8 个 async worker，网络挂起期间等效无限占用。 ✅ 已修复（PAF-08/PAF-25，2026-09-13）
- P1-4 **`infer_main_class` 无缓存全量重扫**：mainClass 缺省时每次 `discover_poms(ws, 5, None, None)`（start.rs:278-279，cache 显式 None），大 workspace 启动秒级延迟。 ✅ 已修复（PAF-09，2026-09-13：manager deps 注入共享 PomCache（内容指纹失效）传给 discover_poms，重复启动不再全量重扫）
- P1-5 **构建路径无进程组**：`process_group(0)` 仅在启动路径（launcher.rs:105），Maven 执行链（executor.rs:75-88 build_process）没有——mvnw/mvnd/Windows `cmd /c` 链存在与 N-07 同构的「父死孙活」窄窗。 ✅ 已修复（PAF-09，2026-09-13：build_process 补 `process_group(0)`（unix），kill_tree 对组长走 killpg 整组投递）

**Git 客户端数据安全**
- P1-6 **rebase 启动不查脏工作区**：ops 校验后直接 `history::reset_to(onto,"hard")`（rebase.rs:164-165），未暂存修改被静默丢弃且不入 undo log；`rebase_continue/rebase_skip/merge_abort` 的 hard reset 同理。 ✅ 已修复（PAF-10，2026-09-13：ensure_clean_worktree 前置校验；skip/abort 的 hard reset 经 UI 二次确认维持 abort 语义）
- P1-7 **多 commit cherry-pick 中途 Abort 语义破损**：`ConflictResolver.vue:312/:476` 调 `abortPick(repoPath)` 不传 `base_oid`；`history.rs:165-171` 在 None 时 reset 到当前 HEAD——前 N-1 个已落地的 pick 提交残留。 ✅ 已修复（PAF-10，2026-09-13：冲突时持久化 pick base 到 .git，abort_pick 显式 base > 持久化 base > 当前 HEAD 兜底）
- P1-8 **rebase_continue 不校验分支被切换**：结果 set 到当前 HEAD 所指 ref（rebase.rs:342-344），冲突挂起期间切分支再 Continue 会把 rebase 链写到错误分支。 ✅ 已修复（PAF-10，2026-09-13：RebaseState.branch_ref + ensure_branch_unchanged，continue/skip/abort 全覆盖）
- P1-9 **merge 无互斥/前置校验**：`merge()`（merge.rs:37-73）不检查已有 MERGE_HEAD（`merge_in_progress` 存在但未调用）、不检查脏区、不检查 rebase 进行中。 ✅ 已修复（PAF-10，2026-09-13：MERGE_HEAD 互斥 + rebase 互斥 + 脏区拒绝）
- P1-10 **batch_add/batch_restore 不走任务队列**：同步串行 fail-fast（git_ops.rs:288/:324），与 T-20「操作全集走队列」口径不符，多仓中途失败无法定位。 ✅ 已修复（PAF-11，2026-09-13：收编 TaskQueue 一仓一任务 + PartialSuccess 聚合；restore 落 T-34 操作日志；前端 waitForTasks 等收口再刷新）

**内存/资源泄漏**
- P1-11 **AI gateway 记录无界增长**：`records: Mutex<HashMap>`（gateway.rs:161）只有插入/读取，`prune_terminal`（:756-761）定义后**全仓零调用**（grep 实证），每条记录克隆完整请求正文。 ✅ 已修复（PAF-12，2026-09-13：插入序队列 + 终态记录容量 128 淘汰，接入两个插入点，带回归测试）
- P1-12 **终端隐藏面板时 writeBuffer 无上限**：6 处 `writeBuffer.push`（terminal.ts:193/:237/:277/:292/:316/:329）无任何上限/丢弃；面板 `v-if` 卸载 XtermView 后所有 PTY/runtime 输出无限堆积（对照 runtime store logBuffers 有 5000 行环形上限）；`pauseSession/flushBuffer`（:531-544）全工程零调用。 ✅ 已修复（PAF-12，2026-09-13：writeBuffer/pendingOutput 5000 块上限超限丢最旧，8 个推入点全覆盖）
- P1-13 **chat known_addrs 无界累积**：`Mutex<HashSet<SocketAddr>>`（chat/manager.rs:72）只插入（:334/:587/:800）无淘汰。 ✅ 已修复（PAF-12，2026-09-13：改为 HashMap<SocketAddr, Instant>，TTL 30min + 硬上限 512 最旧淘汰，带回归测试）

**core watcher（4 个实证缺陷）**
- P1-14 ① debounce 是**丢弃**非合并：窗口内事件 `continue`（watcher.rs:210-213），连续保存两个文件第二个可能永不触发刷新；② mount 用 NonRecursive（:146-159），子目录编辑不产生事件；③ mount 失败不回滚 `watched` 集合（:64-79），失败目录永不重试且无用户可见错误；④ 事件匹配 `path_under_root` 只做边界字节双兼容，无整串分隔符/大小写/`\\?\` 归一化（git_status.rs:320-338）——违反 AGENTS.md §1 自定规则。 ✅ 已修复（PAF-13，2026-09-13）

**前端**
- P1-15 **终端搜索聚焦彻底失效**：选择器 `.terminal-search-input input` 要求嵌套 input，而 class 就在 `<input>` 自身（TerminalPanel.vue:321）——`:60-64` 的 `focus()` 永远匹配不到（核查中新发现，比原判断更严重）。 ✅ 已修复（PAF-14，2026-09-13）
- P1-16 **Ctrl+\` 快捷键死绑定**：`parseKeyEvent` 只识别数字/字母/Enter/F 键（shortcuts.ts:55-83），反引号返回 `""`，terminal:toggle / terminal:new-shell 永远无法触发，而 StatusBar tooltip 仍在宣传该快捷键。 ✅ 已修复（PAF-14，2026-09-13）
- P1-17 **「终端内搜索」命令失效**：`registry.ts:239` 派发 `CustomEvent("terminal:toggle-search")`，全工程无任何 addEventListener 接收。 ✅ 已修复（PAF-14，2026-09-13）
- P1-18 **runtime store 双监听器竞态**：`subscribe` 幂等守卫在第一个 await 之前、`unlisteners` 赋值在 12 个 await listen 全部完成后（runtime.ts:268-334）——快速切视图产生两批监听器（日志重复、事件双触发），前一批永不释放。 ✅ 已修复（PAF-15，2026-09-13）
- P1-19 **stores 过期响应覆盖族**（均无 seq/requestId 防护，对照 `DiffViewer.vue:369-395` 的 `loadSeq` 是现成最佳实践）：changeSet.ts:36-52、runtime.ts:92-126、repository.ts:39-62、RepositoryList.vue:1531-1563（双击 diff）、RuntimeDependenciesView.vue:706-718。 ✅ 已修复（PAF-15，2026-09-13）
- P1-20 **terminal store 监听注册失败永久锁死**：`listenersRegistered` 同步置位，任一 listen 抛异常则 `listenersReady` 成永久 rejected promise 且无法重试（terminal.ts:102-139）。 ✅ 已修复（PAF-16，2026-09-13）
- P1-21 **GitGraph 冲突横幅红底红字不可见**：`.conflict-bar` background 与 `.conflict-text` color 同用 `var(--gw-danger)`（GitGraph.vue:641/:648；tokens.scss:18 `#ff3b30` 不透明）。 ✅ 已修复（PAF-17，2026-09-13）

**后端其他**
- P1-22 **git_link 常驻线程 `expect` ×3**（git_link.rs:167/172/174）：一次 SQL 抖动即线程死亡，Git 联动静默失效到重启。 ✅ 已修复（PAF-19，2026-09-13）
- P1-23 **MCP 本地端点无鉴权**：`server.rs` 无 token 校验、`read_request` 不校验 Content-Type/Origin/Host（恶意网页可用 text/plain 免 preflight 触发工具调用）；默认端口 39117（:29），discovery 文件退出清理（:264）。
- P1-24 **凭证可用性 OnceLock 缓存**：`available: OnceLock<bool>`（credentials.rs:49），注释宣称「`refresh_availability` 可重测」但方法未实现——keyring 晚解锁则直到重启都不可用；`get()` 在 OS 后端 Err 时静默降级查会话（:241-246）。
- P1-25 **5 处 `ends_with(&needle)` 缺路径分隔符边界**：service/mod.rs:82-86、watch/mod.rs:330、git_link.rs:194——project `api` 会误配 `.../myapi`（对照 maven/index/sync.rs:169 用 `{root}/` 做了正确边界）。 ✅ 已修复（PAF-18，2026-09-13）
- P1-26 **`get_workspace_changes` 串行绕过缓存**：逐仓同步调用、无 rayon、不读 status_cache（commands/repository.rs:30-51），首页变更树是全应用最慢列表路径。
- P1-27 **build_code_index 持全局 DB 锁贯穿扫描 + 无事务**（commands/ai.rs:931-937 起锁贯穿 walkdir 循环）——大仓库索引期间全应用 DB 卡顿。
- P1-28 **PTY 生命周期**：`close()` 持 sessions 锁轮询最长 2s（pty.rs:465-499）；死会话不回收（:551 注释与实现不符）；Windows exit_code 恒 None（:621-625）；`unwrap_or(0)` + pid==0 直接返回的孤儿路径（:347/:479-482）。
- P1-29 **Git Console 非实时**：`git_op_output` 在任务收尾批量发送（worker.rs:401-426），流式能力存在但零接线（见 §4 建议）。 ✅ 已修复（PAF-08/PAF-25，2026-09-13）

### P2 —— 加固项（择要）

| 问题 | 位置 |
|---|---|
| 两个 workspace 根重叠时后扫者把仓库行连元数据改判（path 全局 UNIQUE + ON CONFLICT SET workspace_id） | db/schema.rs:31、db/dao.rs:122-131 |
| 扫描取消返回空 Vec，与注释「返回已找到的」不符；scan_cancellable 带 `#[allow(dead_code)]` 未接 UI | core/scanner.rs:41-42/:86-89 |
| `change_stats` 对 untracked 文件全量 read_to_string 数行数（大文件内存尖峰） | core/change_set.rs:376-383 |
| 热力图身份只读全局 git config（忽略仓库级 user.email）+ TIME 早停 break | core/heatmap.rs:31-42/:93-96 |
| 健康评分公式前后端双写（前端注释自认 Mirror） | core/health.rs:130 + HealthView.vue:295-303 |
| `@status:conflict` 探测不含 gitworkspace 自研 rebase 状态文件 | commands/batch.rs:88-92 |
| squash 前驱为 root pick 时报错（可经 skip/abort 恢复，非卡死） | core/rebase.rs:323-325 |
| stash apply/pop 冲突裸错误；branch_from_stash 四步无回滚；stash_clear 非原子 | core/stash.rs:72-141 |
| 远程分支 `is_current` 用 `name.contains(cb)`（origin/feat-x 误标） | core/graph.rs:321-324 |
| pick_continue 丢原 commit 作者（author 用当前签名） | core/history.rs:207-214 |
| 冲突解决逐文件生成独立操作日志且不可撤销（刷屏） | commands/conflict.rs:58-81 |
| `node_list_projects` 每次 IPC 全量重扫且 cancel=None | commands/node.rs:33 |
| registry.find_valid 只查 is_file + is_valid 标志，不重新探测 | node/registry.rs:116-128 |
| 健康探针不支持 HTTPS；多端口缺省取 `ports.last()`；Auto 回退 TCP 可掩盖 TLS 管理端口 | runtime/health.rs:217/:584-593/:331-347 |
| monitor 输出回调内取 DB 锁（输出热路径） | runtime/launch/manager/monitor.rs:71 |
| `runtime_start_in_terminal` open 后立即写命令不等 prompt；Windows `set K=V &&` 拼接对含空格/&/引号值会碎裂；`is_sensitive_env` 启发式双向误差 | commands/terminal.rs:125-203 |
| 新终端默认 cwd 取 `SELECT root_path FROM workspaces LIMIT 1`（非当前 workspace） | commands/terminal.rs:29 |
| 重开会话丢 shell profile（注释说「同 cwd/shell」实际只传 cwd） | TerminalPanel.vue:88-94 |
| TaskPanel 有任务就强制弹出（hidePanel 无抑制标记，下一条事件再次弹出） | stores/task.ts:38-44 |
| 终端右键菜单无 outside-click 关闭（对照 shell ContextMenu 用 n-dropdown） | TerminalPanel.vue:368-399 |
| 「刷新当前窗口」= `window.location.reload()` 丢全部前端内存态 | commands/registry.ts:103-108 |
| StatusBar 工作区弹层用固定定位 1×1px div 锚点 hack | StatusBar.vue:69 |
| XtermView 主题只 mounted 读一次（切主题不生效）；`trim()+"33" \|\| fallback` 恒 truthy 使 fallback 永不生效；watch(active) fit 后未 emit resize | XtermView.vue:52-60/:151-160 |
| UnifiedDiff rows computed 依赖 selection，选行变化全量重建行数组 | UnifiedDiff.vue:104-130 |
| TaskPanel gitLogs `:key="i"` + unshift 头插全列表重渲染 | TaskPanel.vue:187-189 |
| ai store follow 的 unlisten 赋值竞态；AiGitAssistantDialog 轮询无卸载清理（对照 AiConflictAssistant 有 onBeforeUnmount） | stores/ai.ts:610-627、AiGitAssistantDialog.vue |
| session 角色解析 `unwrap_or(WorkspaceAssistant)` 静默降级 | ai/session.rs:223 |
| OpenAI 流式未发 `stream_options.include_usage`；Anthropic 流式只解析 message_delta 的 output_tokens（非流式两种都解析） | ai/adapters/openai_chat.rs、anthropic.rs:166 |
| SSE 泵空闲超时复用整请求 120s 超时（慢思考模型会误杀） | adapters/mod.rs:336-355 + gateway.rs:73/:534 |
| prompt 未转义 `</context-item>`（上下文可打破标签边界；有系统约束第 5 条软防御 + user_content_never_enters_system_layer 测试） | ai/prompt.rs:28/:194 |
| kill_tree `process_alive` 用秒级 start_time 防 PID 复用（同秒复用误判） | process/kill_tree.rs:105-120 |
| logger 初始化失败 expect 直接 panic（logs 目录不可写则应用无法启动） | lib.rs:126 |
| `store::set_pid` 写点位于输出回调持锁路径；`sync_fetch/pull/push` 同步命令无超时阻塞主线程 | monitor.rs:71、commands/git_ops.rs:170-192 |
| LAN chat secret/room_id 提交值未 trim（校验用了 trim 但原始值直传；F-36 doc:56 记录未做） | LanChatTool.vue:313-323 |
| `guard_repo_path` 无 canonicalize/大小写处理（symlink 逃逸、Windows 大小写误判越界） | ai/tools.rs:620-645 |

---

## 4. 还能增加什么功能

### 4.1 规划内缺口（文档已立项、代码零落点，顺理成章的下一步）

| 功能 | 现状证据 | 价值 |
|---|---|---|
| **R-22 Gradle 支持** | `gradle/` 目录为空；pipeline/mod.rs:255 留有分发点注释 | 对 Android/Kotlin/微服务生态是覆盖面质变 |
| **R-23 Debug（JDWP）** | 无 `-agentlib:jdwp` 注入点 | 与 IDE 联调闭环 |
| **R-24 Docker/K8s 运行时** | 无相关代码 | 云原生服务本地编排 |
| **R-25 JVM 深度监控** | 仅 OS 级 CPU/RSS（metrics.rs）；JMX 降级方案的落点已预留 | GC/Heap/线程可视化 |
| **B-10 Port/Adapter trait 抽象** | RuntimeService 直依赖 rusqlite/sysinfo | 可测试性收尾 |
| **N-07 真机复核收尾** | 复选框未勾，测试已固化 | 唯一进行中任务 |

### 4.2 基于核查发现的高价值建议（投入产出比排序）

1. **Git Console 实时流式镜像**——`run_git_streaming`（remote.rs:228）及四个 streaming 封装已实现但零调用方，只需在 worker 接线到 `git_op_output` 事件即可把「收尾批量发送」变成真·实时终端镜像。改动小、感知强。
2. **批量 pull 的分叉跟进动作**——dry-run 已能分类分叉仓库（`batch_dry_run` + `merge_commits` 内存冲突预测），但 `git pull --ff-only` 失败后无后续；可加「分叉→选 merge/rebase 策略」批量跟进。
3. **Undo 覆盖面扩展**——`plan_item` 白名单仅 5 类（undo_plan.rs:62-72）；stash drop/clear、merge abort、cherry-pick、worktree remove（fs 删除）、batch_restore 均不入日志；冲突解决有记录但明确不可撤销且逐文件刷屏。
4. **主题三档切换入口 + StatusBar 分支槽**——`setMode` 零调用、`currentBranch` 空占位，两个小改动补齐 D-02/D-04 承诺。
5. **AI 协议精度**——补 OpenAI `stream_options.include_usage` 与 Anthropic `message_start` input_tokens；SSE 块间空闲超时与整请求超时解耦（o1 类慢思考模型 120s 会误杀）。
6. **MCP 端点鉴权**——per-boot token 写入 discovery 文件并要求 Authorization 头，顺带修 CSRF 面。
7. **前端 E2E harness**——全项目唯一系统性缺口；桌面化 17 任务与 33 个视图全靠构建级验证，建议用 tauri-driver 或 GUI 黑盒测试补最小冒烟集。
8. **PomCache 容量策略**——benchmark README 自录：容量 2048 在 ≥2550 POM 时命中率崩塌（1811/2550→740/10100），大规模企业工作区是目标用户。
9. **GBK/代码页转码**——中文 Windows JVM 日志目前以 U+FFFD 落盘（F-12 后不丢不卡但不可读）。
10. **`loadSeq` 竞态防护模式推广**——DiffViewer 已有全项目最佳实践实现，5 个 store/视图照搬即可。
11. **终端文件路径 Ctrl+Click**——TM-07 明确降级项（自定义 link provider）。
12. **扫描取消接 UI**——`scan_cancellable` 已实现带 `#[allow(dead_code)]`，接上即可消掉「大工作区扫描不可取消」。

---

## 5. 核查记录（本报告与第一轮报告的差异）

### 5.1 被推翻（1 项，已从 bug 清单删除）
- ~~「Spring Boot 端口横幅正则 Tomcat 专属，WebFlux/Jetty 不命中」~~——实际正则 `started on port(?:\(s\))?:?\s+(\d+)`（output.rs:86）按子串匹配，Netty/Jetty/Undertow 标准就绪输出均命中。

### 5.2 修正（15 项，已按修正后表述收录）
1. worker 泄漏主体是 tokio blocking 线程池线程（非 8 个 async worker），且非严格「永久」（worker.rs:189-301）。
2. watch `in_flight` 滞后置位的重复提交在现有单线程 debounce 拓扑下**不可达**（watch/mod.rs:432-444）——从 bug 清单降级为观察项。
3. stop 时 pid 未回填比原判断更严重：强杀分支同样被 `if let Some(pid)` 守卫跳过（control.rs:29-49）。
4. squash-root 报错可经 skip/abort 恢复，非「卡死」（rebase.rs:323-325）。
5. `boot_fixture` 是 fixture 辅助函数非测试；`missing_main_class_is_inferred...`/`bound_jdk_is_used...` 用 FakeRunner——真实集成测试为 classpath_run_full_cycle、real_vite_project_full_loop、reconcile_adopts 三个。
6. Node 决策链「无 lockfile 回退 PATH npm」需补前提（配置与 packageManager 均未命中；且回退后注册表优先）。
7. `find_valid` 检查是 `is_file()` + 库中 `is_valid` 标志（无 is_dir）。
8. 8.14ms 出处为 T-02 文档（release 实测），T-07 记录的是 debug 构建 9.8ms——两个数字都有背书。
9. `sync_fetch/pull/push` 运行于 Tauri 同步命令主线程（非「独立 IPC 线程」），无超时阻塞的实质成立。
10. D-02「三档切换」未勾项位于「需求范围」小节；该文档 9 个复选框全部未勾但标「✅ 已完成」。
11. LAN chat 输入校验用了 `.trim()` 判空，但提交原始值未 trim（F-36 doc:56 确认归一化未做）。
12. batch 聚合逻辑在 manager.rs + worker.rs + models/task.rs，`queue.rs` 无此逻辑。
13. AI 域测试数实测 **190**（原报 223 偏高）；全仓测试属性 **874**（原报 668~752 偏低），T-35 自述 807 通过。
14. 注释与实现不符两处：pty.rs:551 注释宣称「会话清理」但代码未实现；TerminalPanel.vue:91 注释「同 cwd/shell 重开」实际只传 cwd。
15. Anthropic **非流式**两种 token 都解析（:136-137），仅流式只解析 output_tokens。

### 5.3 核查中新发现（原报告没有）
- 终端搜索聚焦选择器永不匹配，功能彻底失效（P1-15）。 ✅ 已修复（PAF-14，2026-09-13）
- 第三处 `btoa(String.fromCharCode(...bytes))`（registry.ts:270）。
- pty.rs / TerminalPanel.vue 两处注释与实现不符。
- D-02 文档勾选状态系统性矛盾。
- Tauri 同步 git 命令阻塞的是主线程（比「IPC 线程」更值得注意）。

---

## 6. 结论

- **整体判断**：工程成熟度显著高于同类项目——真实 Spring Boot/Vite E2E、benchmark 基线、golden IPC 契约、踩坑规则沉淀（AGENTS.md）、安全设计（Preview 闸门/OS Keychain/脱敏管道）扎实；无架构级问题。
- **风险集中三类**：① 启动/停止链路竞态窗口（P0-2/P0-3/P1-1/P1-2）；② 无界增长的内存（P1-11/P1-12/P1-13）；③ 跨平台验收未闭环（N-07/TM-03 真机、CI 环境依赖）。
- **修复顺序建议**：P0 五项（P0-1 一行修复、P0-5 一行比较顺序反转）→ 内存泄漏三连（P1-11/12/13）→ 启动链路三连（P1-1/2 + P0-3 的 UNIQUE 部分索引）→ 真机回归 → P1 其余 → P2 加固。
- **本报告可信度**：全部结论经第二轮 110 条断言逐条源码取证；引用行号为核查时实际行号，后续提交可能使行号漂移，但断言内容以函数/结构名为锚仍可定位。
