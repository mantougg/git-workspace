# T-04 Diff & Graph 硬化

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-03 SQLite 数据层硬化](./T-03-sqlite-data-layer.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 基础稳定化 |
| 优先级 | P0（前置） |
| 状态 | 🟦 进行中 |
| 依赖 | T-03 |
| 对应 Roadmap | §9 Diff 增强、§10 History、§40 缓存架构 |

## 目标

将现有 `core/diff.rs`（Unified/Side-by-Side）与 `core/graph.rs`（SVG 泳道图）硬化：diff 结果缓存、diff 显示设置、大仓库渐进加载。

## 需求范围

- [x] Diff 缓存：以 `(path, old_oid, new_oid)` 为 key（T-12 落地：revision/commit 等不可变对象 diff 进 AppState 有界 LRU，key=(repo, old_oid, new_oid, flags)，容量 32；工作区 diff 不缓存——失效复杂，由 watcher 增量刷新兜底）
- [x] Diff 设置：Ignore Whitespace / Ignore EOL / Ignore Case（`DiffConfig` + `get_workdir_diff_with_config`）
- [x] 大文件保护（untracked/added）：超大 diff 截断（`MAX_FULL_FILE_LINES` 2000 行 + 截断标记，`full_add_hunk_for_file`）
- [x] 大文件保护（tracked 修改）：`extract_hunks` 行数截断（当前 tracked 文件修改无行数上限，全量过 IPC；与验收「超大 diff 不卡死」直接相关）
- [x] Graph 渐进加载：已有分页加载（README 确认）
- [x] Graph 数据缓存：commit 元数据 / 图结构落 SQLite（`upsert_commits_batch` + `get_commit_record` + command 读缓存省 find_commit）
- [x] 前端渲染预算：diff 视图 / 变更树虚拟滚动或分页，单屏 DOM 上限 + 帧时间测量（Roadmap 评审增量，见全局约束 §2）
- [ ] 二进制定位与降级提示（剩余：P2，T-30）

## 架构 / 性能注意点

- Diff 属于重计算，缓存命中率直接决定查看体验；缓存上限走 LRU，与 T-02 状态缓存共用策略。
- Graph 构建是 CPU 密集，与 status 并发争抢时需遵守全局并发限流。
- 大 diff 传输走分页 / 流式，避免单条 IPC 携带 MB 级 payload 阻塞 UI。

## 验收标准

- [ ] 同一文件二次查看 diff < 50ms（缓存命中，T-07 实测）（缓存命中路径已过微基准：1000 次命中 <50ms；T-07 harness 端到端实测待补）
- [x] Graph 分页加载 + 落库缓存实现（代码完成，`cargo test` 验证）
- [ ] 大仓库（10k+ commit）Graph 首屏 < 1s（T-07 实测）
- [x] Ignore Whitespace 等设置切换即时生效且结果正确
- [x] 超大 diff 不再导致 UI 卡死（untracked/added 2000 行截断 + tracked 修改 `extract_hunks` 2000 行预算截断 + 前端 Unified/Side-by-Side 虚拟滚动单屏 DOM 上限）

## 进度

### 状态

- 当前状态：进行中（剩余：Graph 10k 首屏实测待 T-07、二进制定位待 T-30）
- 最近更新：2026-08-14 revision diff LRU 缓存随 T-12 落地

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：DiffConfig（ignore whitespace/EOL/case）+ 大文件截断（2000 行）；get_diff command 加 options 参数；`cargo check` 通过。剩余：diff LRU 缓存（待 T-12）、Graph 落库
| 2026-08-13 | 🟦 | 完成 Graph 数据落库与缓存：commits/commit_parents 落库 + command 读缓存省 find_commit；`cargo test` 35 passed。剩余：diff LRU 缓存（待 T-12）、二进制定位（待 T-30）
| 2026-08-13 | ⏸️ | 完成 diff 显示设置前端接入与验证：DiffViewer 加 Ignore Whitespace/EOL/Case 开关即时重载 + `getDiff` 加 `options`；修复 `ignore_case` 语义（libgit2 `GIT_DIFF_IGNORE_CASE` 仅文件名比较，改为内容级后处理过滤）+ 补 5 个 diff 单元测试；`cargo test` 41 passed、`vue-tsc --noEmit` 通过。剩余：diff LRU 缓存（验收「二次查看 <50ms」）待 T-12、二进制定位待 T-30，转 ⏸️ |
| 2026-08-13 | 🟦 | 回退：新增前端渲染预算需求（diff 视图虚拟滚动 + 帧时间测量，Roadmap 评审增量）；验收 4 重新打开待前端渲染预算完成 |
| 2026-08-13 | 🟦 | 完成 tracked 修改截断 + 前端渲染预算：`extract_hunks` 每文件 2000 行预算 + 截断标记（常量统一为 `MAX_DIFF_LINES_PER_FILE`，补 tracked 截断单元测试）；新增 `VirtualList`（固定行高窗口化）改造 Unified/Side-by-Side diff（hunks 扁平化为行列表，单屏 DOM 有界）；新增 `utils/frameTime.ts` rAF 帧时间测量通道（`window.__gwPerf` + 慢帧告警）并接入 DiffViewer；`cargo test` 44 passed、`vue-tsc` + `vite build` 通过。验收「超大 diff 不卡死」闭环 |
| 2026-08-14 | 🟦 | diff LRU 缓存落地（随 T-12）：revision/commit 等不可变对象 diff 进 AppState 有界 LRU（key=repo+old_oid+new_oid+flags，容量 32），缓存命中微基准 1000 次 <50ms；工作区 diff 不缓存（watcher 增量兜底）。剩余：Graph 10k 首屏 T-07 实测、二进制定位（T-30） |

### 子任务清单

- [x] 实现 diff 结果缓存（LRU）（随 T-12 落地：revision/commit 对象 diff；工作区 diff 不缓存）
- [x] extract_hunks 行数截断（tracked 修改文件）
- [x] 前端渲染预算（diff 视图虚拟滚动 + 帧时间测量）
- [x] 实现 diff 显示设置
- [x] 实现 Graph 分页与懒加载（已有，确认）
- [x] Graph 数据落库与缓存
