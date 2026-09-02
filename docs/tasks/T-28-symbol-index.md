# T-28 Tree-sitter Symbol Index

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-03 SQLite 数据层硬化](./T-03-sqlite-data-layer.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 4 · Code Intelligence（P2） |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | T-03 |
| 对应 Roadmap | §26 Symbol Index、§25 Workspace Code Search、§41 schema（symbols / references） |

## 目标

在现有 FTS5 全文索引之上增加代码结构索引（Tree-sitter），支持符号/定义/引用搜索与调用层级。

## 需求范围

- [x] 基于 Tree-sitter 建立 Symbol / Function / Class / Struct / Interface / Method / Variable / Reference
- [x] Symbol Search / Definition Search / Reference Search / Call Hierarchy
- [x] Go To Definition / Find References
- [x] 落库 `symbols` / `references`（与 `code_index` FTS5 互补）
- [x] 搜索过滤：`@repo:` / `@group:` / `@ext:` / `@path:` / `@status:`

## 架构 / 性能注意点

- Tree-sitter 解析是 CPU 密集，索引构建走 §45 Index 4 并发限流，且增量重建（仅重解析变更文件）。
- 符号索引与 FTS5 分离存储，查询按需 join；1000 仓库下禁止全量重建，必须有增量 + 缓存。

## 验收标准

- [x] 多语言（按需至少覆盖主要语言）符号索引正确
- [x] Go To Definition / Find References 结果准确
- [x] 索引构建增量进行，单文件变更只重解析该文件
- [x] Symbol Search 响应 < 100ms（索引内）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发。方案：tree-sitter 0.25 + rust/ts/tsx/js/py/go/java 语法 0.23；`symbols` 表扩列（end_line/container/signature）+ 新增 `symbol_refs`（按名引用，is_call 区分调用）+ `symbol_index_files`（内容 hash 增量）；查询走 name 基 join（定义/引用/调用层级）；过滤 @repo/@group/@status 复用批量选择器 facet，@ext/@path 走 SQL LIKE |
| 2026-09-02 | ✅ | 完成。新增 `src-tauri/src/symbols/`（lang：按扩展名选语法与查询；extract：查询捕获 + kind/容器推断（impl/class/receiver 祖先）；index：SHA-256 hash 增量重建、按文件事务替换、定义/引用/调用层级查询（最深容器相关子查询）、过滤解析）；`commands/symbols.rs` 五个命令（build_symbol_index / search_symbols / find_symbol_definitions / find_symbol_references / symbol_call_hierarchy）；batch.rs 抽出 `facet_repo_paths` 供 @status 复用；前端 `api/symbols.ts` + 符号搜索视图（路由 /symbols，Git 分组）。边界说明：引用按 (name,line) 去重、不解析类型/作用域（LSP 级语义留待后续）；前端无编辑器跳转表面，Go To Definition / Find References 以准确定位数据 + 符号搜索视图呈现。验证：`cargo test --lib` 792 通过（symbols 15 项：多语言提取/增量单文件重解析/定义引用调用层级/过滤/10k 符号 <100ms）；`pnpm build` 通过 |

### 子任务清单

- [x] Tree-sitter 集成与符号提取
- [x] symbols / references 落库
- [x] Definition / References / Call Hierarchy 查询
- [x] 增量重建与过滤搜索
