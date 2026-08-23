# R-11 Runtime 日志引擎

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 构建运行闭环 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | R-10 |
| 对应源文档 | §35 日志系统、§36 日志功能、§37 日志智能增强（预留）、§77 Runtime Log Secret Mask |

## 目标

统一接管构建与应用进程的 stdout / stderr / application.log，提供实时滚动、过滤、搜索、导出的日志能力，并全程 Secret 脱敏。

## 需求范围

- [x] 日志源接管（§35）：进程 stdout / stderr 实时捕获，落盘 `.gitworkspace/logs/`；支持读取应用自身 `application.log`（只读 `search_file` / `tail_file`，不改用户文件）
- [x] 日志功能（§36）：实时滚动 / 暂停 / 清空 / 搜索 / 复制 / 导出 / 级别过滤（INFO / WARN / ERROR / DEBUG）——其中「暂停 / 复制」是 UI 交互，归 R-13；本任务交付数据面的实时推送 / 清空 / 搜索 / 导出 / 级别过滤
- [x] 级别识别：主流日志格式（Logback / Log4j2 默认 pattern）的 level 解析，识别不出时降级为原文
- [x] Secret 脱敏（§77）：`password=123456` 形态整段打码；敏感环境变量值在日志中一并打码（全局约束 §4，与 T-08 共用规则）
- [x] 背压与资源控制：环形内存缓冲 + 批量聚合推送 UI；日志文件滚动切分与容量上限
- [x] 为 §37 智能增强（Exception Detection / Stack Trace Folding / Error Highlight）预留解析接口位（`LogAnalyzer` trait + `register_analyzer`），本任务不实现

## 架构 / 性能注意点

- 日志洪水场景（Spring Boot 启动刷数千行）不得打爆 UI：发送端聚合节流，UI 侧虚拟滚动（前端渲染预算沿用全局约束）。
- 捕获线程与进程生命周期绑定，进程死即收；落盘写入用批量 flush，不逐行 sync。
- 脱敏在**落盘前**完成，保证磁盘上无明文 secret；脱敏规则与 T-08 共用。
- 大文件搜索/导出走流式读取，不整文件载入内存。

## 验收标准

- [x] 应用启动日志实时可见，级别过滤/搜索/导出可用且导出内容与显示一致
- [x] 高频输出下 UI 保持响应（事件聚合生效）
- [x] 脱敏规则单测覆盖（含环境变量值、key=value 形态）
- [x] 磁盘日志文件无未脱敏 secret（测试断言）
- [x] 进程结束后日志完整保留、可回查

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-23 完成开发（验收全部通过）

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-23 | 🟦 开始开发 | 启动 R-11：进程 stdout/stderr 捕获落盘 `.gitworkspace/logs/`（滚动切分）、级别解析（Logback/Log4j2 + 降级）、脱敏管道（落盘前，复用 T-08）、环形缓冲 + 聚合推送、搜索/导出/清空；为 §37 智能增强预留解析接口位 |
| 2026-08-23 | ✅ 完成开发 | 新模块 `runtime/logs/`（level/redact/engine）：线程模型 = `LogSession::log` 捕获路径只做脱敏+级别解析+mpsc 发送，每会话一个 worker 线程按 `aggregate_interval`（默认 100ms）聚合——批量写盘（BufWriter 64KB，每批 flush 不逐行 sync）→ 有界环形缓冲（2000 行/512KB）→ `LogAnalyzer`（§37 接口位）→ 分块（≤256 行/事件）发 `RuntimeEvent::Logs`。落盘 `<workspace>/.gitworkspace/logs/<runtime>/<pid>.log`，滚动切分 8MiB×3 段（容量上限），目录创建沿用 reject_symlink 守卫 + runtime 名路径校验。脱敏统一为 `LogRedactor`（T-08 `mask_secrets` + 敏感环境值 ≥4 字符替换 `MASKED_VALUE`），R-09 pipeline 的 `sensitive_values`/`mask_line` 已收敛到 `runtime/logs/redact.rs`（单一规则来源，全局约束 §4）；构建行到达会话前已被 pipeline RedactingSink 脱敏，会话侧再脱敏为幂等防御。manager 接入：Start 时 `open_log_session`（秘密值取自五层合并环境，仅内存持有；日志目录不可写 fail-fast 终止 Start），`BuildLogSink` 替换 NullSink 接管构建输出，monitor `on_line` 接 Run 阶段输出，终态路径统一 `finish_session`（monitor 先收口日志再发布终态，`abort_before_spawn` 幂等兜底）——构建+运行输出同文件、终态即可完整回查。查询面：`search`/`tail`/`export`/`clear` 全流式（不整文件载入），export 与 search 共用过滤管道（导出=显示）；`search_file`/`tail_file` 只读接管应用自身 application.log；级别过滤不淘汰 None 级行（stack trace 续行可见）。已实现的聚合节流语义：事件速率 ≤ 1/interval，稀疏行 ≤1 interval 内推送。注意点：`OutputStream` 补 serde derive（`stdout`/`stderr`）；`RuntimeEvent::Logs` 变体 camelCase IPC-ready 但 golden 快照注册与 Tauri 桥接按先例留 R-12；「暂停/复制」是 UI 交互归 R-13。验证：`cargo check --all-targets` 干净；`cargo clippy --all-targets --all-features` 改动文件零警告；`cargo test runtime::logs` 22 passed（解析矩阵/脱敏含环境值与 key=value/背压聚合/滚动切分/磁盘无明文/导出一致/回查/清空/只读 application.log/分析器挂接/路径守卫）；`cargo test runtime::launch` 32 passed（含新集成：fake 全闭环构建+运行输出脱敏落盘、Logs 事件两阶段覆盖、端口探测回归、结束后 search 回查）；`cargo test runtime::build` 30 passed（RedactingSink 重构回归）；`cargo test` 全量 377 passed / 2 ignored（`maven::settings::tests::resolve_uses_settings_when_present` 为本机 `~/.m2/settings.xml` 显式设置 localRepository 导致的既有环境性失败，与 R-11 无关，同 R-10 记录） |

### 子任务清单

- [x] 进程输出捕获 + 落盘（滚动切分）
- [x] 级别解析器（多格式 + 降级）
- [x] 脱敏管道（落盘前）
- [x] 环形缓冲 + 聚合推送
- [x] 搜索 / 导出 / 清空
- [x] 单元测试（解析 / 脱敏 / 背压）
