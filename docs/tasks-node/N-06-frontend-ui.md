# N-06 前端 UI 接入（Wizard / Dashboard / api）

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§4.8 / §4.9](../node-frontend-runtime-design.md)；根 `AGENTS.md` Desktop Skin 约定（tokens / 骨架组件）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · UI 与端到端验收 |
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 依赖 | N-03, N-04 |
| 对应设计文档 | §4.8 IPC 与契约、§4.9 前端 UI |

## 目标

配置向导支持创建「前端工程」运行时（选 node 项目 + script + 包管理器），Dashboard 正确展示 node 配置；事件订阅零改动。

## 需求范围

- [ ] `RuntimeAppWizard.vue`：首步加运行时类型选择（Spring Boot / 前端工程）；node 分支项目选择器改取 `node_list_projects`；script 下拉（所选项目 `scripts`）；包管理器选择（默认「自动推断」）；jdk/profile/closure 等 JVM 字段按 kind 隐藏
- [ ] `RuntimeDashboard.vue`：「启动方式」列对 `kind=node` 降级显示为 `<pm> run <script>`；其余列复用
- [ ] `src/api/node.ts` 封装（`node_list_projects` 等）；`src/types/runtime.ts` 同步 `kind` 字段
- [ ] 编辑既有 springBoot 配置的向导行为不变（回归）
- [ ] **不做**统一项目视图抽象（设计文档 §4.8）：两个并列 IPC，wizard 内按 kind 分源取数

## 架构 / 性能注意点

- 样式一律 `--gw-*` tokens，等宽路径/script 名用 `--gw-font-mono`；不硬编码色值像素。
- `scripts` 下拉数据来自 `node_list_projects` 返回的 `scriptsJson` 前端反序列化；顺序保持 package.json 原序。
- stores/runtime.ts 事件订阅不应需要任何改动（事件流类型无关）；若发现需要改，停下来核对设计文档。

## 验收标准

- [ ] 向导可创建 node 配置：选项目 → 选 script → 保存 → 列表展示 `kind=node`
- [ ] 保存后重新打开向导回显正确（roundtrip）
- [ ] Dashboard node 行显示 `<pm> run <script>`；启动/停止/日志按钮可用
- [ ] springBoot 配置创建/编辑回归无变化
- [ ] `pnpm build` / `pnpm tsc`（或项目既有前端检查命令）通过

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 | 开始开发：Wizard 增加 Runtime 类型与 Node 项目/script/包管理器分支，Dashboard 接入 Node 启动方式展示。 |
| 2026-08-31 | ✅ | 完成：Wizard 支持 Spring Boot/前端工程切换、Node 项目和 script 原序选择、自动/显式包管理器及编辑回显；JVM 字段按 kind 隐藏；Dashboard 结合配置详情与 node_projects 显示 `<pm> run <script>`。`pnpm build` 通过，本地页面切换分支冒烟通过。 |

### 子任务清单

- [x] api/node.ts + types 同步
- [x] Wizard 类型选择与 node 分支表单
- [x] Dashboard 启动方式列降级
- [x] 回显/回归验证
