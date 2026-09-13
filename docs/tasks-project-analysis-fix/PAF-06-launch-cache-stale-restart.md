# PAF-06 launch_cache 只插不清，改配置后「重启」静默用旧 LaunchPlan

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 修改端口/JDK/vm_options 后 Restart 生效新配置
- [ ] 缓存正确命中场景不回归（无配置变更时 Restart 复用产物）
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
