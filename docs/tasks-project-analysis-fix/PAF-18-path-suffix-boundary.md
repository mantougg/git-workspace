# PAF-18 后端路径匹配缺组件边界（ends_with 三处 + guard 大小写/verbatim）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-25，核查智能体验证 |
| 关联任务 | AGENTS.md §1 平台兼容规范、R-15/R-17/R-21 |

## 问题描述

1. 三处 `path == needle || path.ends_with(&needle)` 缺 `/` 组件边界：
   - `runtime/service/mod.rs:82-86`
   - `runtime/watch/mod.rs:330`
   - `runtime/git_link.rs:194`

   project 名是其他路径段后缀时（project `api` 会匹配 `.../myapi`），选中
   错误模块——watch 影响分析、自动重启、分支联动作用到错误对象。对照组
   `maven/index/sync.rs:169` 用 `format!("{root}/")` 做了正确边界。
2. `runtime/guard.rs:19/:33` 写护栏 `Path::starts_with` 是组件级匹配（边界
   安全），但无大小写处理、无 `\\?\` verbatim 前缀清洗——Windows/macOS
   大小写不敏感 FS 上一侧大小写不同会误报 Permission。

## 定位与修复建议

- 统一改为 `path == needle || path.ends_with(&format!("/{needle}"))`，或
  提取公共「路径组件级后缀匹配」函数三处复用；
- guard.rs 比较前做归一化（分隔符 + verbatim 剥离；大小写策略按 AGENTS.md
  §1 约定并文档化）。

## 验收标准

- [x] project `api` 不再匹配 `myapi` 目录（单测：`pathutil::component_match_requires_full_component_suffix` + `find_project_tests::api_does_not_match_myapi_directory`）
- [x] guard 在大小写/verbatim 混合路径下不误报（`allows_verbatim_backslash_paths` + cfg 门控大小写用例）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据全部成立。按任务文档建议提取公共模块 `src-tauri/src/pathutil.rs`：`strip_windows_verbatim_prefix`（收口 maven/node/git_status 三份副本）、`normalize_for_compare`（verbatim+分隔符+cfg 大小写折叠）、`path_component_match`（组件序列级后缀匹配）。三处 `ends_with` 调用点（service `find_project` / watch `runtime_closure` / git_link `dirty_for_app`）改走 `path_component_match`——组件序列比较同时修复多段 needle（`backend/api` 误配 `xbackend/api`）与 Windows 盘符路径等值匹配。`guard.rs` 新增 `guard_starts_with`：比较前归一化（保留组件级边界语义）；**影响分析 CRITICAL**（全部 Runtime 写路径护栏）已按纪律评估——归一化只对比较更宽松，合法路径行为不变，既有护栏测试全数通过并新增 4 个用例。验证：`cargo test --lib`（pathutil 4 / guard 5 / find_project 2 / node::scan 9 / maven::index 8 / core::git_status 6 全过；benchmark_smoke 墙钟抖动为既有问题见 PAF-26）。 |
