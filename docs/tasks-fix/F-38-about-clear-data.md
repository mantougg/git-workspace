# F-38 关于页增加「清除数据」功能（二次确认）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-04 用户反馈（第 1 项） |
| 关联任务 | T-35（关于页诊断与反馈区块）、R-02（SQLite 索引） |

## 需求描述

在「关于」页增加清除数据功能：清除已保存在本地 SQLite 表中的数据，
操作前需用户二次确认。

## 范围（与用户确认）

**清除：历史与缓存类（可重建 / 纯历史）**；**保留：所有手动配置**。

- 清除：
  - 运行历史：`runtime_processes`、`task_history`、`operation_logs`、
    `operation_log_items`
  - 仓库索引缓存：`commits`、`commit_parents`、`commit_files`、`branches`、
    `remote_branches`、`tags`、`stashes`、`worktrees`、`repo_status`、
    `file_status`
  - 符号索引：`symbols`、`symbol_references`、`symbol_refs`、
    `symbol_index_files`
  - Maven 索引：`maven_projects`、`maven_dependencies`、`maven_modules`、
    `maven_artifacts`、`maven_source_mappings`
  - Runtime 依赖索引：`runtime_dependencies`（FK 自 maven_projects 级联，
    可由「解析依赖」重建）
  - AI 历史/缓存：`ai_reviews`、`ai_tasks`、`ai_sessions`、`ai_messages`、
    `ai_requests`、`ai_result_cache`、`ai_proposals`
- 保留（配置/用户数据）：`workspaces`、`repositories`、`repo_groups`、
  `tasks*`、`change_sets*`、`workspace_stashes*`、`runtime_projects`、
  `jdks`、`maven_executables`、`node_*`、`ai_providers`、`ai_models`、
  `ai_settings`、`ai_task_defaults`、`plugin_actions`、`scheduled_tasks`

## 实现

- 后端：新命令 `clear_cached_data`（`commands/app.rs`），单事务逐表
  `DELETE FROM`，返回各表清除行数；不删库文件、不动配置表。
- 前端：AboutView 新增「数据」区块，红色按钮 + `dialog.error` 二次确认
  （列出将清除的类别），成功后 message 提示清除行数。

## 验收标准

- [x] 二次确认后才执行；取消不产生任何改动（`dialog.error` + onPositiveClick 才调命令）
- [x] 清除后配置类数据（工作区/JDK/Runtime 配置等）完好（CLEARABLE_TABLES 白名单 + 保护表单测 `clearable_tables_exclude_config_and_user_data`）
- [x] 重新扫描/解析依赖后索引类数据可重建（清的都是索引/历史表）
- [x] `cargo test`（commands::app）+ `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-04 修复完成，测试与构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-04 | ⬜ | 用户反馈录入：关于页加清除数据功能（二次确认） |
| 2026-09-04 | 🟦 | 范围确认：清历史与缓存、保留配置；开始实施 |
| 2026-09-04 | ✅ | 后端 `clear_cached_data` 命令（commands/app.rs，单事务逐表 DELETE + 行数返回 + 保护表白名单单测），前端 AboutView「数据」区块 + dialog.error 二次确认。验证：cargo test + pnpm build 通过 |
