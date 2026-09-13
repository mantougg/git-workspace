# GitWorkspace 项目分析报告修复任务总览（PAF 系列）

> 来源：2026-09-13 项目全景分析报告 [docs/project-analysis-2026-09-13.md](../project-analysis-2026-09-13.md)
> 事实核查批次（125 条断言逐条源码取证：111 成立 / 1 推翻 / 13 修正后收录）。
> 拆分原则：**每个问题一个独立文档**（同目录下 `PAF-XX-<slug>.md`），可独立跟踪修复进度与验收。
> 本文件是唯一的修复进度索引；每个任务文档内另有自己的「进度」章节。
>
> 与用户反馈修复（`docs/tasks-fix/` F 系列）的区别：本系列全部来自分析报告
> 的主动核查，任务文档内附已实证的 file:line 证据。
>
> 横切约束不重复记录：Git 功能相关遵守 [docs/tasks/00-全局开发约束.md](../tasks/00-全局开发约束.md)，Runtime 相关遵守 [docs/tasks-runtime/00-全局开发约束.md](../tasks-runtime/00-全局开发约束.md)，平台兼容性遵守根目录 `AGENTS.md` 的「平台兼容性开发规范」。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 修复中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**26**
- 已完成：**22** · 修复中：**0** · 未开始：**4**

---

## 任务索引

| 编号 | 问题 | 优先级 | 状态 | 文档 |
|---|---|---|---|---|
| PAF-01 | 中文 commit message 触发 UTF-8 字节边界 panic（worker.rs 按字节切片） | P0 | ✅ | [PAF-01-commit-message-utf8-panic.md](./PAF-01-commit-message-utf8-panic.md) |
| PAF-02 | Runtime spawn 超时后进程存活但记录终态，不可停止（孤儿进程） | P0 | ✅ | [PAF-02-runtime-spawn-timeout-orphan.md](./PAF-02-runtime-spawn-timeout-orphan.md) |
| PAF-03 | Runtime 重复启动守卫 TOCTOU，并发产生双进程 | P0 | ✅ | [PAF-03-runtime-start-toctou.md](./PAF-03-runtime-start-toctou.md) |
| PAF-04 | 终端 btoa spread 大文本栈溢出（3 处） | P0 | ✅ | [PAF-04-terminal-btoa-spread-overflow.md](./PAF-04-terminal-btoa-spread-overflow.md) |
| PAF-05 | GitGraph「加载更多」只生效一次（先赋值再比较恒 false + O(n²) 重拉） | P0 | ✅ | [PAF-05-gitgraph-load-more-once.md](./PAF-05-gitgraph-load-more-once.md) |
| PAF-06 | launch_cache 只插不清，改配置后「重启」静默用旧 LaunchPlan | P1 | ✅ | [PAF-06-launch-cache-stale-restart.md](./PAF-06-launch-cache-stale-restart.md) |
| PAF-07 | stop 在 pid 未回填时强杀也是 no-op，restart 撞 Stopping 报 Conflict | P1 | ✅ | [PAF-07-stop-pid-none-restart-conflict.md](./PAF-07-stop-pid-none-restart-conflict.md) |
| PAF-08 | git 网络任务超时后 spawn_blocking 线程无取消机制 | P1 | ✅ | [PAF-08-git-network-task-timeout-leak.md](./PAF-08-git-network-task-timeout-leak.md) |
| PAF-09 | infer_main_class 无缓存全量重扫 + Maven 构建路径无进程组 | P1 | ✅ | [PAF-09-main-class-cache-build-process-group.md](./PAF-09-main-class-cache-build-process-group.md) |
| PAF-10 | Git 操作数据安全前置校验（rebase 脏区 / 分支切换 / merge 前置 / cherry-pick abort） | P1 | ✅ | [PAF-10-git-op-safety-prechecks.md](./PAF-10-git-op-safety-prechecks.md) |
| PAF-11 | batch_add / batch_restore 收编任务队列（部分失败语义 + 操作日志） | P1 | ✅ | [PAF-11-batch-add-restore-task-queue.md](./PAF-11-batch-add-restore-task-queue.md) |
| PAF-12 | 无界内存增长三连（AI gateway records / 终端 writeBuffer / chat known_addrs） | P1 | ✅ | [PAF-12-unbounded-memory-trio.md](./PAF-12-unbounded-memory-trio.md) |
| PAF-13 | core watcher 四缺陷（debounce 丢弃 / NonRecursive 盲区 / mount 不回滚 / 路径未归一化） | P1 | ✅ | [PAF-13-core-watcher-four-defects.md](./PAF-13-core-watcher-four-defects.md) |
| PAF-14 | 终端快捷键与命令失效三连（Ctrl+` 死绑定 / 搜索命令无人监听 / 搜索聚焦选择器失效） | P1 | ✅ | [PAF-14-terminal-shortcut-search-broken.md](./PAF-14-terminal-shortcut-search-broken.md) |
| PAF-15 | stores 过期响应覆盖与 runtime 双监听器竞态（loadSeq 模式推广） | P1 | ✅ | [PAF-15-store-race-loadseq.md](./PAF-15-store-race-loadseq.md) |
| PAF-16 | terminal store 监听注册失败永久锁死 | P1 | ✅ | [PAF-16-terminal-store-listener-deadlock.md](./PAF-16-terminal-store-listener-deadlock.md) |
| PAF-17 | GitGraph 冲突横幅红底红字不可见 | P1 | ✅ | [PAF-17-gitgraph-conflict-bar-invisible.md](./PAF-17-gitgraph-conflict-bar-invisible.md) |
| PAF-18 | 后端路径匹配缺组件边界（ends_with 三处 + guard 大小写/verbatim） | P1 | ✅ | [PAF-18-path-suffix-boundary.md](./PAF-18-path-suffix-boundary.md) |
| PAF-19 | git_link 常驻线程 expect ×3，SQL 抖动即线程死亡 | P1 | ✅ | [PAF-19-git-link-thread-expect.md](./PAF-19-git-link-thread-expect.md) |
| PAF-20 | MCP 本地端点无鉴权（per-boot token + 请求头校验） | P1 | ✅ | [PAF-20-mcp-endpoint-auth.md](./PAF-20-mcp-endpoint-auth.md) |
| PAF-21 | 凭证可用性 OnceLock 缓存 + get 静默降级 | P1 | ✅ | [PAF-21-credential-availability-oncelock.md](./PAF-21-credential-availability-oncelock.md) |
| PAF-22 | get_workspace_changes 串行且绕过缓存（首页最慢路径） | P1 | ⬜ | [PAF-22-workspace-changes-serial.md](./PAF-22-workspace-changes-serial.md) |
| PAF-23 | build_code_index 持全局 DB 锁贯穿扫描且无事务 | P1 | ⬜ | [PAF-23-code-index-db-lock.md](./PAF-23-code-index-db-lock.md) |
| PAF-24 | PTY 生命周期加固（close 持锁 2s / 死会话回收 / pid=0 / 注释不符） | P1 | ⬜ | [PAF-24-pty-lifecycle-hardening.md](./PAF-24-pty-lifecycle-hardening.md) |
| PAF-25 | Git Console 实时流式接线（run_git_streaming → git_op_output） | P1 | ✅ | [PAF-25-git-console-realtime-streaming.md](./PAF-25-git-console-realtime-streaming.md) |
| PAF-26 | P2 加固项批次清单（约 40 项，触及文件时顺手修或拆分） | P2 | ⬜ | [PAF-26-p2-hardening-batch.md](./PAF-26-p2-hardening-batch.md) |

---

## 维护规范

1. 修复任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成修复需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 根因 + 修法 + 验证命令）。
3. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
4. 任务文档内的 file:line 证据来自 2026-09-13 核查，**修复前先以函数名/结构名为锚复核**；若证据已不成立（已被其他提交修复），在时间线注明并将任务置 ⏸️。
5. 完成修复后，在 [project-analysis-2026-09-13.md](../project-analysis-2026-09-13.md) §3 对应条目行尾追加「✅ 已修复（PAF-XX，日期）」，保持报告与本索引一致。
6. 批次编排（修复顺序、任务簇协同）见 `.agents/skills/gitworkspace-project-analysis-fix/SKILL.md`。
