# PAF-09 infer_main_class 无缓存全量重扫 + Maven 构建路径无进程组

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-4 / P1-5，核查智能体验证 |
| 关联任务 | R-06（Spring Boot 检测）、N-07（进程组修复参照）、R-18 |

## 问题描述

两项启动链路问题：

1. `runtime/launch/manager/start.rs:278-281` `infer_main_class` 在 mainClass
   缺省时每次 `discover_poms(ws, 5, None, None)`——cache 参数显式传 None，
   不经 PomCache/SpringBootDetectionCache，大 workspace 启动延迟秒级。
2. `maven/executor.rs:75-88` `build_process` 未设 `process_group(0)`（对比
   启动路径 launcher.rs:105 有），unix 下 mvnw 链/mvnd/Windows
   `cmd /c mvnw.cmd→java` 存在与 N-07 同构的「父死孙活」窄窗——构建取消时
   孙子进程可能滞留。

## 定位与修复建议

1. 主类推断结果纳入缓存（复用 SpringBootDetectionCache 的内容指纹失效
   机制，或写入 launch_cache 上游）；
2. unix 下构建进程同样 `process_group(0)`，取消路径先 killpg（参照
   kill_tree.rs 既有实现）。

## 验收标准

- [ ] mainClass 缺省时重复启动不再全量重扫（可用日志/计时验证）
- [ ] 构建取消实测杀掉整棵进程树（参照 cancelling_real_maven_build_kills_process_tree）
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
