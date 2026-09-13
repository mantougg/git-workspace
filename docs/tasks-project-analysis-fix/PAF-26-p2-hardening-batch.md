# PAF-26 P2 加固项批次清单

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）§3 P2 表，全部经核查 |
| 关联任务 | 各 PAF-XX 触及文件时顺手拆分独立任务 |

## 问题描述

分析报告中实证的 P2 级加固项汇总。每一项都附源码位置；建议触及对应文件
时顺手修复，或按需拆分为独立 PAF-XX 任务（拆分后从本清单划去并互相引用）。

## 加固清单

### 工作区引擎

- [ ] 两个 workspace 根重叠时后扫者把仓库行连元数据改判
      （`db/schema.rs:31` path 全局 UNIQUE + `db/dao.rs:122-131` ON CONFLICT
      SET workspace_id）——改复合唯一或拒绝重叠 workspace
- [ ] 扫描取消返回空 Vec 与注释不符；`scan_cancellable` 带
      `#[allow(dead_code)]` 未接 UI（`core/scanner.rs:41-42/:86-89`）
- [ ] `change_stats` 对 untracked 文件全量 read_to_string 数行数，大文件
      内存尖峰（`core/change_set.rs:376-383`）
- [ ] 热力图身份只读全局 git config（忽略仓库级 user.email）+ TIME 排序
      早停 break（`core/heatmap.rs:31-42/:93-96`）
- [ ] 健康评分公式前后端双写（`core/health.rs:130` 与
      `src/views/HealthView.vue:295-303`，注释自认 Mirror 需人工同步）
- [ ] `@status:conflict` 探测不含自研 rebase 状态文件
      gitworkspace-rebase.json（`commands/batch.rs:88-92`）
- [ ] watcher `last_refresh` 只增不减、3 处 `lock().unwrap()` 中毒级联风险
      （`core/watcher.rs:65/:81/:179/:190`）

### Git 客户端

- [ ] squash 前驱为 root pick 时报错（可 skip/abort 恢复，非卡死；
      `core/rebase.rs:323-325`）
- [ ] stash apply/pop 冲突裸错误无结构化处理；branch_from_stash 四步无回滚；
      stash_clear 非原子（`core/stash.rs:72-141`）
- [ ] 远程分支 `is_current` 用 `name.contains(cb)` 误标 origin/feat-x
      （`core/graph.rs:321-324`）
- [ ] pick_continue 丢原 commit 作者（`core/history.rs:207-214`）
- [ ] 冲突解决逐文件生成独立操作日志且不可撤销，刷屏 OperationLogView
      （`commands/conflict.rs:58-81`）
- [ ] CommitGraph 泳道布局只看已加载页，翻页 lane 重排「跳动」
      （`src/components/graph/CommitGraph.vue`）

### Runtime / Node

- [ ] `node_list_projects` 每次 IPC 全量重扫且 cancel=None
      （`commands/node.rs:33`）
- [ ] registry.find_valid 只查 is_file + is_valid 标志，不重新探测可执行性
      （`node/registry.rs:116-128`）
- [ ] 健康探针不支持 HTTPS；多端口缺省取 `ports.last()`；Auto 回退 TCP 可
      掩盖 TLS 管理端口（`runtime/health.rs:217/:584-593/:331-347`）
- [ ] monitor 输出回调内取 DB 锁写端口（`launch/manager/monitor.rs:71`）
- [ ] kill_tree `process_alive` 用秒级 start_time 防 PID 复用，同秒复用误判
      （`process/kill_tree.rs:105-120`）
- [ ] logger 初始化失败 expect 直接 panic（`src-tauri/src/lib.rs:126`）
- [ ] `sync_fetch/pull/push` 同步命令无超时阻塞主线程
      （`commands/git_ops.rs:170-192`）
- [ ] wait_with_timeout 轮询期间不读管道，超 64KB 输出阻塞至超时
      （`maven/detect_exec.rs:283-318`）
- [ ] PomCache 容量 2048 在 ≥2550 POM 淘汰（benchmarks/README.md:28 自录）
- [ ] unix reparent 场景端口 PID 归属确认不出（信息缺失非误判，
      kill_tree.rs:122-129 注释自认）

### 终端 / 前端

- [ ] `runtime_start_in_terminal` open 后立即写命令不等 prompt；Windows
      `set K=V &&` 拼接对含空格/&/引号值碎裂（`commands/terminal.rs:125-203`）
- [ ] `is_sensitive_env` 启发式双向误差（漏短 secret / 误拦长 hex）
      （`commands/terminal.rs:181-203`）
- [ ] 新终端默认 cwd 取 `SELECT root_path FROM workspaces LIMIT 1` 非当前
      workspace（`commands/terminal.rs:29`）
- [ ] 重开会话丢 shell profile（注释「同 cwd/shell」实际只传 cwd；
      `TerminalPanel.vue:88-94`）
- [ ] TaskPanel 有任务就强制弹出（hidePanel 无抑制标记；`stores/task.ts:38-44`）
- [ ] 终端右键菜单无 outside-click 关闭（`TerminalPanel.vue:368-399`，对照
      shell ContextMenu 用 n-dropdown）
- [ ] 「刷新当前窗口」= `window.location.reload()` 丢全部前端内存态
      （`commands/registry.ts:103-108`）
- [ ] StatusBar 工作区弹层 1×1px 固定定位锚点 hack（`StatusBar.vue:69`）
- [ ] XtermView：主题只 mounted 读一次；`trim()+"33" || fallback` 恒 truthy
      fallback 永不生效；watch(active) fit 后未 emit resize
      （`XtermView.vue:52-60/:151-160`）
- [ ] UnifiedDiff rows computed 依赖 selection，选行变化全量重建
      （`UnifiedDiff.vue:104-130`）
- [ ] TaskPanel gitLogs `:key="i"` + unshift 头插全列表重渲染
      （`TaskPanel.vue:187-189`）
- [ ] ai store follow 的 unlisten 赋值竞态；AiGitAssistantDialog 轮询无卸载
      清理（对照 AiConflictAssistant.vue:327-329 有 onBeforeUnmount）
- [ ] 原生控件技术债：TerminalPanel 12 处、SideNav 2、ToolboxView 2、
      TerminalTabs 2、UnifiedDiff 2、ConflictResolver 1、GitCheatSheet 1
      （AGENTS.md 桌面皮肤规范）

### AI

- [ ] session 角色解析 `unwrap_or(WorkspaceAssistant)` 静默降级
      （`ai/session.rs:223`）
- [ ] OpenAI 流式未发 `stream_options.include_usage`；Anthropic 流式只解析
      message_delta 的 output_tokens（非流式两种都解析）
      （`ai/adapters/openai_chat.rs`、`anthropic.rs:166`）
- [ ] SSE 泵空闲超时复用整请求 120s，慢思考模型误杀
      （`ai/adapters/mod.rs:336-355` + `ai/gateway.rs:73/:534`）
- [ ] prompt 未转义 `</context-item>`，上下文可打破标签边界（有系统约束
      第 5 条软防御；`ai/prompt.rs:28/:194`）
- [ ] `guard_repo_path` 无 canonicalize/大小写处理（symlink 逃逸、Windows
      大小写误判越界；`ai/tools.rs:620-645`）
- [ ] LAN chat secret/room_id 提交值未 trim（F-36 doc:56 记录未做；
      `LanChatTool.vue:313-323`）

## 验收标准

- [ ] 清单项被逐项处理（修复 / 拆分为独立 PAF-XX / 明确记录不修的理由）

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入（P2 汇总） |
