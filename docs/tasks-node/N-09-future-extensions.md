# N-09 展望：monorepo / bun / watch 联动 / 模板

> **开发前必读**：本目录 [00-全局开发约束.md](./00-全局开发约束.md)；设计文档 [§7 P3 / §9](../node-frontend-runtime-design.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 增强与展望 |
| 优先级 | P2 |
| 状态 | 🟦 进行中（2026-09-02 用户触发） |
| 依赖 | N-08 + 触发条件（已满足） |
| 对应设计文档 | §7 P3 展望、§9 开放问题 |

## 触发条件

**无真实用户需求不启动。** 触发示例：用户工作区出现 npm workspaces monorepo 且需要按子包启动；用户环境以 bun 为主；明确要求前端工程随源码变更自动重启。触发后先把具体场景记入本文件时间线，再拆子任务。

**2026-09-02 触发记录**：用户逐项确认范围——monorepo **做**、bun 可执行 **做**、watch 联动 **不做**、模板 **做**；开放问题决策：**做**统一项目视图（Maven/Node 合并列表）、node 配置**纳入** R-15 Runtime Environment 分组启停。

## 目标（已按触发裁剪）

- monorepo：npm workspaces / pnpm workspaces 子包发现与 script 路由（根 `package.json` 的 `workspaces` 字段 + 子包 scripts）
- bun：`bun.lockb` 从「只识别」升级为可执行（`bun run <script>`），含显式 `bun install`
- ~~watch 联动~~：用户确认不做（dev server 自带 HMR，联动价值存疑，正好规避）
- 模板：R-19 Runtime Templates 增加 node 类型模板
- 开放问题决策（用户已定）：做统一项目视图（Maven/Node 合并列表）；node 配置纳入 R-15 Runtime Environment 分组启停

## 架构 / 性能注意点

- monorepo 发现**复用** `node_projects` 表（子包各占一行），不建新表；路由逻辑放决策链扩展。
- dev server 进程常驻且自带 HMR，R-17 联动默认应为「不自动重启」，避免与 HMR 语义打架。

## 验收标准

- [x] monorepo：workspaces 解析纯函数（npm/yarn `workspaces` 字段 + pnpm-workspace.yaml 朴素解析 + `*`/`**` 段匹配）5 单测通过；安装路由到 workspace 根（`install_dir_for`，子包装根/独立装原地）有单测；`node_list_projects`/统一列表返回 `workspaceRoot`
- [x] bun：检测/决策链/命令形状全链路可执行——`bun_decision_resolves_like_other_managers`、`bun_install_runs_real_loopback`（真实 bun install）、`bun_engine_launches_real_dev_script`（真实 bun run dev 闭环）通过；注册表可登记 bun；Wizard 增加 bun 选项
- [x] 模板：`builtin_templates()` 增加「Node.js Frontend Development」；`save_config_as_template` 的 `applies_to` 按 kind 推导（不再硬编码 spring-boot）
- [x] 统一项目视图：新 IPC `runtime_list_unified_projects`（扁平结构 + node/maven payload），golden 样例与 TS 类型登记，Wizard node 分支切换为统一列表取数并展示 workspace 归属
- [x] R-15 分组启停纳入 node：端口覆盖按 kind 分流（node → `PORT` 环境变量，不注入 `--server.port=`；jdk/profile 覆盖对 node 忽略并记日志）；`environment_start_covers_node_service_with_port_override` 通过
- [x] watch 联动：用户确认不做，已从范围移除
- [x] 四件套 + 前端构建全绿（全量 `cargo test` 752 通过 0 失败；`GW_UPDATE_GOLDEN=1` 重新生成快照并复验；`pnpm build` 通过；改动文件 fmt/clippy 清零）

## 进度

### 状态

- 当前状态：已完成（代码与验收全绿；2026-09-02 提交）
- 最近更新：2026-09-02

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 用户触发并确认范围：monorepo / bun / 模板 / 合并列表 / R-15 分组启停 做；watch 联动不做。开始开发 |
| 2026-09-02 | ✅ | 全部完成。bun 放开 3 处拒绝关卡（detect/安装 IPC/注册表）+ 真实 bun install/run dev 闭环测试；新增 `node/workspace.rs`（workspaces 解析 + 段匹配 + 向上找根 + 安装路由，5 单测）；`NodeProjectNode` 增 `workspaceRoot`（列表时向上推断）；新 IPC `runtime_list_unified_projects`（Maven/Node 合并列表，golden + TS 登记，Wizard node 分支接入并显示 workspace 归属）；内置模板增 Node 项、`applies_to` 按 kind 推导；R-15 端口覆盖按 kind 分流（node 走 PORT env）+ node 分组启动冒烟测试。全量 `cargo test` 752 通过 0 失败，golden 重新生成复验通过，`pnpm build` 通过，改动文件 fmt/clippy 清零（仓库其余为预存基线） |

### 子任务清单

- [x] monorepo：workspaces 解析（根 + 子包）与安装路由，复用 `node_projects` 表
- [x] bun 可执行：检测/决策链/命令形状/显式 install 放开
- [x] 模板：R-19 Runtime Templates 增加 node 类型模板
- [x] 统一项目视图：Maven/Node 合并列表（IPC + golden + TS + Wizard 接入）
- [x] R-15 分组启停纳入 node 配置（端口覆盖按 kind 分流）
- [x] 四件套 + pnpm build + golden 同步 + 回归
