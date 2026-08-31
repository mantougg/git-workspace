# N-03 Runtime 配置模型扩展（kind / node_script）

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§4.3](../node-frontend-runtime-design.md)；`../tasks-runtime/00-全局开发约束.md` §8（配置分层 / 向后兼容）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 1 · 配置与启动闭环 |
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 依赖 | N-02 |
| 对应设计文档 | §4.3 Runtime 配置模型扩展、§4.7（ScriptNotFound） |

## 目标

`RuntimeApplicationConfig` 新增 `kind` / `node_script` / `node_package_manager` 字段，`runtime_projects` 表加 `kind` 列；保存时按 kind 校验；历史 springBoot 配置零迁移。

## 需求范围

- [ ] 配置字段（全部 `#[serde(default)]`）：`kind: RuntimeKind`（`springBoot` \| `node`，缺省 springBoot）、`node_script: Option<String>`、`node_package_manager: Option<String>`
- [ ] `CURRENT_SCHEMA_VERSION` 升版；旧 JSON 加载语义不变（回归测试锁定）
- [ ] `SCHEMA_V18` 迁移：`runtime_projects ADD COLUMN kind TEXT NOT NULL DEFAULT 'springBoot'`
- [ ] 保存校验：`kind=node` 必须有 `node_script` 且存在于目标 package.json 的 `scripts`（否则 `ScriptNotFound`，列出可用 scripts）；`kind=springBoot` 时 node 字段必须为 `None`
- [ ] `kind=node` 时 `project` 列存 package.json 目录 path（复用 V11 既有列语义）
- [ ] `RuntimeConfigSummary` 加 `kind`（serde 缺省兼容）；golden 快照重新生成

## 架构 / 性能注意点

- `RuntimeKind` 用 serde 字符串枚举，拒绝未知值时给可行动错误（`RuntimeConfig` + 当前支持列表）。
- script 存在性校验依赖 N-02 的发现数据：按 `project` path 读 `node_projects.scripts_json`；库中无记录时回退到磁盘直读 package.json（防索引过期误拒）。
- `environment` / `runtime_environment` / `program_arguments` / `health_check` 等既有字段对 node 原样可用，不新增语义分叉。

## 验收标准

- [ ] 旧配置 JSON（无 kind 字段）加载后 `kind == springBoot`，保存往返无损
- [ ] `kind=node` 缺 script / script 不存在 / springBoot 带 node 字段 三类校验错误均可行动
- [ ] V18 迁移幂等；既有行 `kind` 默认值为 `springBoot`
- [ ] golden 快照更新；`RuntimeConfigSummary.kind` 前端可用
- [ ] 四件套全绿

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|

### 子任务清单

- [ ] 配置字段与 RuntimeKind 枚举
- [ ] SCHEMA_V18 迁移 + 幂等测试
- [ ] 保存校验（含 ScriptNotFound）
- [ ] Summary/golden/前端类型同步
- [ ] 四件套验证
