# N-02 package.json 发现与索引

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§4.2](../node-frontend-runtime-design.md)；`../tasks-runtime/00-全局开发约束.md` §5（缓存）/ §7（基础设施复用）/ §8（SQLite 元数据）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · 工具链与发现 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | —（复用 T-01 Scanner / T-03 SQLite） |
| 对应设计文档 | §4.2 package.json 发现与索引 |

## 目标

在 workspace 边界内发现 `package.json`（与 git 解耦，沿用 R-27 补扫语义），解析 `scripts` 等元数据并落 `node_projects` 表，对外提供 `node_list_projects` IPC。

## 需求范围

- [ ] `node/discovery.rs::discover_package_jsons`：workspace 清单扫描 + 根补扫；跳过 `node_modules` / `dist` / `build` / `.git` / dotdir
- [ ] 解析（`serde_json`，无新增 crate）：`name` / `version` / `scripts`（有序 map 原样存 JSON）/ `packageManager` / lockfile 存在性；**不解析 dependencies**
- [ ] `SCHEMA_V17` 迁移：`node_projects` 表（字段见设计文档 §4.2 SQL），`UNIQUE(workspace_id, path)`，path 存 path_key 归一化形式
- [ ] 缓存：`pkg_hash` 内容 hash 未变不重新解析（对齐 POM Cache 语义）
- [ ] IPC：`node_list_projects(workspace_id)` → `NodeProjectNode[]`；新增 `commands/node.rs`；`src/types` + `src/api/node.ts` 封装；golden 快照重新生成（`GW_UPDATE_GOLDEN=1`）

## 架构 / 性能注意点

- 发现走 T-01 Scanner 复用边界，**不另写目录遍历**；与 `maven/discovery.rs` 的跳过目录策略对齐（`node_modules` 本就在跳过集）。
- `scripts_json` 存原文 JSON 串，读取方（wizard）自行反序列化——表结构不随 script 增减而变。
- 迁移测试沿用 `db/schema.rs` 既有模式：旧版本库升级幂等。
- 性能验收：单 workspace 发现 < 500ms（对齐 Runtime 全局约束 §5 发现类指标，以 Benchmark 实测为准）。

## 验收标准

- [ ] fixture workspace（含嵌套 package.json + node_modules 干扰项）发现结果正确
- [ ] `pkg_hash` 未变二次扫描不解析（计数器断言）
- [ ] V17 迁移幂等；`UNIQUE(workspace_id, path)` 重扫不重复插行
- [ ] `node_list_projects` golden 快照更新；前端 `src/api/node.ts` 可调通
- [ ] 发现性能 Benchmark 达标（实测记录入时间线）
- [ ] 四件套全绿

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] discovery 扫描 + 跳过目录
- [ ] package.json 解析纯函数 + 单测
- [ ] SCHEMA_V17 迁移 + 幂等测试
- [ ] `node_list_projects` IPC + golden + 前端 api
- [ ] Benchmark 与四件套验证
