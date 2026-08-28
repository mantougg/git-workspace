# F-15 Runtime 分组无数据 + 无「新建应用」入口

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成（2026-08-28） |
| 来源 | 2026-08-28 用户实测反馈问题 2 |
| 关联任务 | D-01~D-06（ca47b39 删按钮）、R-12/R-13 |

## 问题描述

1. Runtime 总览没有「新建应用」按钮（D-01~D-06 改造时被移除，空态提示文案
   还在引用这个按钮）。
2. release.2 工作区下，Runtime 总览 / 依赖 / 作用域 / 日志全部无数据——
   但 DB 里实际存在该工作区的配置（`runtime_projects` 1 行「960release2」）。

## 根因（已定位，两条独立）

1. **事件订阅异常阻断数据加载**：Runtime 事件名含 `.`（如
   `runtime.project_discovered`，`src/api/runtime.ts:21-35` ↔
   `src-tauri/src/runtime/events.rs:31-43`），Tauri v2 的 `listen` 校验只允许
   字母数字/`-`/`/`/`:`/`_`——第一个 `await listen(...)` 即抛错，
   `RuntimeDashboard.vue` onMounted 中 `await store.subscribe()` 之后的
   `await reload()` 永远不执行 → 总览无数据。控制台可见
   `invalid args 'event' for command 'listen'`。
2. **子视图的 workspaceId 只由 RuntimeDashboard 写入**：
   `stores/runtime.ts:28` 的 `workspaceId` 是独立 ref，只有
   `RuntimeDashboard.vue:939` 调 `setWorkspace`；直接进依赖/作用域/日志视图
   时它为 null，各视图 `reload()` 直接早退 → 永远无数据。

## 修复范围

- [x] Runtime 事件名 `.` → `_`（前后端两侧同步，对齐 watcher 的
  `repo_status_changed_batch` 约定）
- [x] RuntimeDashboard onMounted：先 `reload()` 后 `subscribe()`（订阅失败
  不再阻断数据加载），subscribe 加容错
- [x] `stores/runtime.ts` 的 `workspaceId` 改为从全局 workspace store 派生
  （computed + watch 自动 reloadAll），移除仅 Dashboard 写入的依赖；各 Runtime
  视图 mount 时确保 `loadWorkspaces()` 已执行
- [x] RuntimeDashboard 工具行恢复「新建应用」按钮（`router.push` 到
  `runtime-app-wizard`，对齐 493e41f 被删前的语义）

## 验收标准

- [x] Runtime 总览显示既有配置（960release2）
- [x] SideNav 直接进入依赖/作用域/日志视图，有数据或正常空态（非异常静默）
- [x] 控制台不再出现 `invalid args 'event' for command 'listen'`
- [x] 「新建应用」按钮可进入向导并创建成功
- [x] `pnpm build` 通过；runtime 相关 cargo 测试不回归

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-28 修复完成并实测验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；CDP 实测定位：事件名含 `.` 使 listen 抛错阻断 reload；子视图 workspaceId 依赖 Dashboard 写入 |
| 2026-08-28 | 🟦 | 开始修复 |
| 2026-08-28 | ✅ | 修复：①事件名 `.`→`_`（`api/runtime.ts` + `runtime/events.rs` 两侧常量，对齐 watcher 的 `repo_status_changed_batch` 约定）；②`stores/runtime.ts` 的 workspaceId 改派生自全局 workspace store（computed + immediate watch 自动 reloadAll，置空清空），删除 setWorkspace；`useRuntimeWorkspace` 简化为 loadWorkspaces + 订阅容错（先数据后订阅）；③RuntimeDashboard 工具行恢复「新建应用」按钮。验证（安装包 CDP 实测）：总览显示 960release2 配置 + 2 进程记录 + 2 项目索引，「新建应用」按钮在；直达依赖（17 依赖边/2 模块）/作用域/日志均正常；控制台零错误。`pnpm build` 过；`cargo test real_maven`（JDK17）11 过 |
