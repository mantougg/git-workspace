# AI-07 Git Assistant 公共 Diff 管道

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[AI-03](./AI-03-context-builder-preview.md)（上下文/Preview/Secret）、[T-04](../tasks/T-04-diff-graph.md)（Diff）、[T-08](../tasks/T-08-errors-logging-secrets.md)（Secret）。设计约束见 [ai-assistant-design.md](../ai-assistant-design.md) §14.1。

| 项 | 值 |
|---|---|
| 阶段 | Phase C · Git Assistant |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | AI-03, T-04, T-08 |
| 对应设计文档 | §14.1 公共 Diff 上下文管道 |

## 目标

为 AI-08（Commit/Review/PR/Explanation）与 AI-09（Conflict Resolution）建立**共享**的 Diff 上下文管道：三个场景只保留任务 prompt 与结果 Schema 的差异，不重复实现 HTTP 与安全链路（§1.3 的教训）。

## 需求范围

- [x] 文件级选择与排除：多 Repository、目录、文件三级颗粒度，排除后重算扫描/估算/hash（复用 AI-03）
- [x] 来源标记：`staged / worktree / base / ours / theirs`，每个 ContextItem 可追溯到来源与范围
- [x] diff 结构摘要：文件清单、hunk 结构、增删行数统计，优先于完整 diff 进上下文（§8.2 Code Review / Commit Message 策略）
- [x] 行数与 token 预算：按任务类型套用 AI-03 预算策略，超限截断/排除在 Preview 可见
- [x] Secret Scan 与 Mask/Exclude 接入（复用 AI-03 管道，含二次扫描）
- [x] 输入 hash 与结果缓存接入（复用 AI-04 缓存 Key 规则）
- [x] Preview 与用户确认接入（复用 AI-03 Preview 契约）
- [x] 结构化结果解析框架：各场景注册自己的输出 Schema，解析失败降级为纯文本并标记

## 架构 / 性能注意点

- 代码落点：`src-tauri/src/ai/context.rs` 的 Git 场景装配器 + `src/components/ai/` 的 diff 选择/排除 UI 片段；Diff 数据一律来自 T-04/T-12 现有能力，AI 层不直接操作 git2 句柄。
- 大 diff 分块发送而非全量（§8.2）；多仓库场景按仓库逐个组装 Manifest item。
- 现有 `ai_review` 原型命令在本任务完成后改为走统一管道（保留 IPC 兼容，内部转发；移除模型硬编码与前端传 Key）。

## 验收标准

- [ ] Commit Message / Review / Conflict 三类场景共用同一管道：代码走查确认无第二套 HTTP/Secret/Preview 实现
- [ ] 文件/目录/仓库级排除在 Preview 生效，排除后内容不出现（测试断言）
- [ ] staged/worktree/base/ours/theirs 来源标记正确（单测）
- [ ] diff 内容变化使缓存失效（复用 AI-04 测试）
- [ ] 旧 `ai_review` 命令行为兼容且不再硬编码模型

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 开始开发 | 恢复 AI-07：扩展多仓库 Diff 选择/来源标记，迁移兼容 `ai_review` 到统一 Preview/Gateway。 |
| 2026-08-30 | 完成 | 接入 ChangeSet 多仓库/目录/文件选择与真实 Preview 确认链；统一 Diff、Conflict、Secret、预算、hash、Gateway 和旧 `ai_review` 兼容路径。安全走查确认 AI 层未直接 spawn、操作 Git 写状态或落盘凭证。验证：`cargo check`、`cargo test ai::context::tests::`、`cargo test ai::preview::tests::`、`cargo test ipc_golden`、`pnpm build`。 |

### 子任务清单

- [x] diff 上下文装配器（来源标记 + 结构摘要）
- [x] 文件级选择/排除 UI 片段
- [x] Secret/Preview/缓存三接入
- [x] 结构化结果解析框架
- [x] 旧 `ai_review` 迁移到统一管道
- [x] 单元/集成测试
