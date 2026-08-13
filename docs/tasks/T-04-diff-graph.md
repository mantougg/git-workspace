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

- [ ] Diff 缓存：以 `(path, old_oid, new_oid)` 为 key（剩余：工作区 diff 失效复杂，配合 T-12 对象 diff 一起做）
- [x] Diff 设置：Ignore Whitespace / Ignore EOL / Ignore Case（`DiffConfig` + `get_workdir_diff_with_config`）
- [x] 大文件保护：超大 diff 截断（`MAX_FULL_FILE_LINES` 2000 行 + 截断标记）
- [x] Graph 渐进加载：已有分页加载（README 确认）
- [ ] Graph 数据缓存：commit 元数据 / 图结构落 SQLite（剩余：`commits`/`commit_parents` 表已建，落库逻辑未接）
- [ ] 二进制定位与降级提示（剩余：P2，T-30）

## 架构 / 性能注意点

- Diff 属于重计算，缓存命中率直接决定查看体验；缓存上限走 LRU，与 T-02 状态缓存共用策略。
- Graph 构建是 CPU 密集，与 status 并发争抢时需遵守全局并发限流。
- 大 diff 传输走分页 / 流式，避免单条 IPC 携带 MB 级 payload 阻塞 UI。

## 验收标准

- [ ] 同一文件二次查看 diff < 50ms（缓存命中，T-07 实测）
- [ ] 大仓库（10k+ commit）Graph 首屏 < 1s，分页加载流畅
- [ ] Ignore Whitespace 等设置切换即时生效且结果正确
- [ ] 超大 diff 不再导致 UI 卡死

## 进度

### 状态

- 当前状态：进行中
- 最近更新：2026-08-13 开始开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-13 | 🟦 | 核心完成：DiffConfig（ignore whitespace/EOL/case）+ 大文件截断（2000 行）；get_diff command 加 options 参数；`cargo check` 通过。剩余：diff LRU 缓存（待 T-12）、Graph 落库

### 子任务清单

- [ ] 实现 diff 结果缓存（LRU）（剩余，待 T-12 对象 diff）
- [x] 实现 diff 显示设置
- [x] 实现 Graph 分页与懒加载（已有，确认）
- [ ] Graph 数据落库与缓存（剩余）
