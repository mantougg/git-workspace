# F-23 Runtime 总览加「启动方式」列（直接启动 vs 源码启动）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-28 用户反馈（下午场，配源码依赖场景提问） |
| 关联任务 | R-02（源码映射）、R-03（闭包 / Synthetic Reactor）、R-13（Runtime store） |

## 问题描述

Runtime 总览看不到一个应用是「直接启动」（纯 jar 依赖）还是「源码启动」
（引入了工作区内其他 Maven 项目的源码依赖），用户无法确认配置是否生效。

## 方案（已定位）

判定数据用 **Runtime Closure**——与 Build 流水线决定 reactor 构建范围是同一
数据源（`maven/closure.rs`），语义与实际启动行为一致：

- 闭包 `projects` 含应用根项目自身 + 全部工作区源码依赖；
  过滤 `rootProjectId` 后即源码依赖清单；
- 数量为 0 → 直接启动；> 0 → 源码启动 ×n；
- 闭包有服务端双层缓存（依赖图 fingerprint + closure fingerprint），
  额外成本只是读 N 个配置 JSON，**零后端改动**（复用 `get_runtime_config`
  + `runtime_get_closure`，与 RuntimeScopeView 同模式）；
- 未跑过「解析依赖」（依赖图为空）时闭包只剩根项目，语义上会误显示
  「直接启动」——因此计算失败/无图时统一显示「—」+ tooltip 引导先解析。

## 修复范围

- [x] `src/stores/runtime.ts`：新增 `closureInfo`（runtimeName →
  `{ sourceCount, sourceNames } | null`）+ `loadClosureInfo()`；
  在 `reloadAll`（configs 之后）、`saveConfig`/`removeConfig`（scope 可能变）、
  `dependencyResolved` 事件（依赖图变化）后刷新
- [x] `src/views/RuntimeDashboard.vue`：`configColumns` 在「项目」后插
  「启动方式」列——`直接启动`（灰）/ `源码启动 ×n`（绿，tooltip 列
  artifactId 名单）/ `—`（tooltip 引导先「解析依赖」）

## 验收标准

- [x] 总览每行展示启动方式；源码启动的悬浮可见源码依赖名单
- [x] 与 Scope 视图预览闭包的模块数一致（同一命令同一缓存）
- [x] `pnpm build`（vue-tsc + vite）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 实现完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 需求录入；调查确认闭包即判定数据源、双层缓存成本低、前端零后端改动方案可行 |
| 2026-08-28 | ✅ | 实现 store closureInfo + 总览「启动方式」列；验证：`pnpm build` 通过；UI 实测以用户验收为准 |
