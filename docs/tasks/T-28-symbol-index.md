# T-28 Tree-sitter Symbol Index

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-03 SQLite 数据层硬化](./T-03-sqlite-data-layer.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · Code Intelligence（P2） |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-03 |
| 对应 Roadmap | §26 Symbol Index、§25 Workspace Code Search、§41 schema（symbols / references） |

## 目标

在现有 FTS5 全文索引之上增加代码结构索引（Tree-sitter），支持符号/定义/引用搜索与调用层级。

## 需求范围

- [ ] 基于 Tree-sitter 建立 Symbol / Function / Class / Struct / Interface / Method / Variable / Reference
- [ ] Symbol Search / Definition Search / Reference Search / Call Hierarchy
- [ ] Go To Definition / Find References
- [ ] 落库 `symbols` / `references`（与 `code_index` FTS5 互补）
- [ ] 搜索过滤：`@repo:` / `@group:` / `@ext:` / `@path:` / `@status:`

## 架构 / 性能注意点

- Tree-sitter 解析是 CPU 密集，索引构建走 §45 Index 4 并发限流，且增量重建（仅重解析变更文件）。
- 符号索引与 FTS5 分离存储，查询按需 join；1000 仓库下禁止全量重建，必须有增量 + 缓存。

## 验收标准

- [ ] 多语言（按需至少覆盖主要语言）符号索引正确
- [ ] Go To Definition / Find References 结果准确
- [ ] 索引构建增量进行，单文件变更只重解析该文件
- [ ] Symbol Search 响应 < 100ms（索引内）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] Tree-sitter 集成与符号提取
- [ ] symbols / references 落库
- [ ] Definition / References / Call Hierarchy 查询
- [ ] 增量重建与过滤搜索
