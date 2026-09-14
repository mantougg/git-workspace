# PAF-06 launch_cache 只插不清，改配置后「重启」静默用旧 LaunchPlan

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-1，主控亲自验证 |
| 关联任务 | R-07（Runtime 配置体系）、F-04（启动参数预设） |

## 问题描述

`runtime/launch/manager/mod.rs:119`
`launch_cache: Arc<Mutex<HashMap<(i64, String), CachedLaunch>>>` 的全部引用
只有 insert（mod.rs:275、start.rs:344/:399）与读（mod.rs:289、
start.rs:257/:301），**无任何 remove/失效**；`control.rs:134-145` 的
`restart()` 强制 `skip_build = true` 后 `prepare()` 命中缓存直接返回旧
plan；`commands/runtime.rs` 的 `update_runtime_config` 不失效缓存。
改端口/JDK/vm_options/mainClass 后点「重启」静默用旧参数。

## 定位与修复建议

- `update_runtime_config` / 删除配置 / 模板应用时清除对应 (workspace,
  runtime) 缓存键；
- 或把配置指纹纳入缓存 key（改动自然失效，与 R-06 SpringBootDetectionCache
  的内容指纹思路一致）。

## 验收标准

- [x] 修改端口/JDK/vm_options 后 Restart 生效新配置
- [x] 缓存正确命中场景不回归（无配置变更时 Restart 复用产物）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 证据复核成立（缓存仍只插不清）。按任务文档「配置指纹纳入缓存」方案实施（覆盖面大于逐点失效，天然覆盖 AI 提案 / 端口改写等全部配置写路径）：`CachedLaunch` 增加 `config_fingerprint`（持久化配置 + 本次覆盖项的稳定哈希），`prepare()` 命中判定指纹一致才复用，失配回退完整构建；`delete_runtime_config` 显式清除残留键。回归测试 `config_change_invalidates_launch_cache`：改端口后 Restart 重建（Maven 调用 1→2），无变更 Restart 复用（保持 2）。验证：`cargo test --lib` 881 passed |
