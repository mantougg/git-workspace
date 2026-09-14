# PAF-09 infer_main_class 无缓存全量重扫 + Maven 构建路径无进程组

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
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

- [x] mainClass 缺省时重复启动不再全量重扫（可用日志/计时验证）
- [x] 构建取消实测杀掉整棵进程树（参照 cancelling_real_maven_build_kills_process_tree）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 证据复核成立（两处均未变）。① `RuntimeProcessDeps` 注入 `pom_cache`（RuntimeService 共享同一实例），`infer_main_class` 改传 `Some(&pom_cache)`——PomCache 按内容哈希失效，重复启动 POM 解析走缓存不再全量重扫；② `maven/executor.rs::build_process` 补 unix `process_group(0)`（launcher.rs 启动路径同款），构建取消时 kill_tree 对组长走 killpg 整组投递，关闭「父死孙活」窄窗。验证：`cargo test --lib` 881 passed（含既有 `cancelling_real_maven_build_kills_process_tree` 等构建取消回归） |
