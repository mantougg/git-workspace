# R-08 IDEA 对比测试：场景、口径与结果模板（§98）

> 对比性质：**半自动**——IDEA 侧人工操作 + 计时，GitWorkspace Runtime 侧由 `cargo run --release --example benchmark -- runtime <repos> <modules>` 自动产出。本文件固定测量口径，保证两侧数据可比。
> 数据存档：GitWorkspace 侧结果落盘 `docs/tasks-runtime/benchmarks/runtime_<repos>x<modules>.json`（由 benchmark 子命令自动保存）；IDEA 侧结果手工填入本文末尾的结果表。

## 对比对象与前提

| 项 | 口径 |
|---|---|
| 硬件 | 同一台机器、同一电源策略；两次测量间隔内不跑其他重负载 |
| 工程 | 同一份合成工程（`benchmark::maven_gen` 生成，确定性）或同一份真实工程，两侧都先拷贝到独立目录 |
| IDEA | 记录版本号；关闭无关插件；索引完成后才算"可用" |
| GitWorkspace | release 构建；指标取自 `runtime` 子命令 JSON 字段 |
| 次数 | 每场景 3 次取中位数（首次 Build 只取首趟） |

## 场景与测量口径

| 场景 | IDEA 侧口径 | GitWorkspace 侧口径 | 对应 JSON 字段 |
|---|---|---|---|
| 冷启动导入 | 双击启动 IDEA → Open 工程 → 进度条消失、索引完成，秒表计时 | 冷 Discovery + 依赖解析建索引 | `discovery_cold_ms + index_sync_ms` |
| 热启动导入 | IDEA 已装、工程已导入过：重启 IDEA 打开同一工程 → 可用 | POM Cache 命中的整仓 reload + Graph Cache 命中 | `pom_cache_hit_ms + graph_cache_hit_ms` |
| 首次 Build | IDEA 内 Build Project（清空 `target/` 与编译缓存后） | 待 R-09 Build Engine 接入后测 `mvn package` 全量 | `build_ms`（预留） |
| 二次 Build | 紧接着再次 Build Project（无改动） | 同上，无改动二次构建 | `build_ms`（预留） |
| 修改单模块 | 改一个叶子模块的一个 java 文件 → Build Project 完成 | 待 R-17 增量构建接入 | `build_ms`（预留） |
| 修改底层模块 | 改被最多模块依赖的底层模块 → Build Project 完成 | 同上 | `build_ms`（预留） |
| 多服务启动 | Run 全部 Spring Boot 服务 → 全部端口可达 | 待 R-10 Launcher + R-16 Health 接入 | `app_start_ms`（预留） |
| File Change → Detection | —（IDEA 无对应显式指标） | 文件写入 → watcher 事件到达 | `file_change_detection_ms` |

> IDEA 计时建议：用 IDEA 自带 `Build` 输出的耗时（Build tool window 会打印 `completed in X s Y ms`），启动/导入类用秒表。每次测量记录原始值，不只记中位数。

## 结果记录模板

日期 / 机器 / IDEA 版本 / GitWorkspace commit：

| 场景 | IDEA (ms) | GitWorkspace (ms) | 备注 |
|---|---:|---:|---|
| 冷启动导入 | — | | IDEA 含索引 |
| 热启动导入 | — | | |
| 首次 Build | — | N/A（待 R-09） | |
| 二次 Build | — | N/A（待 R-09） | |
| 修改单模块 | — | N/A（待 R-17） | |
| 修改底层模块 | — | N/A（待 R-17） | |
| 多服务启动 | — | N/A（待 R-10/R-16） | |

## 解读约束

- Build / 启动类场景**不设固定 SLA**（§99），只记录趋势与量级差异。
- IDEA 冷启动含完整代码索引，GitWorkspace 不做代码索引（全局约束 §1 不替代 IDEA）——冷启动口径差异要在备注中显式说明，不做"谁更快"的绝对结论，只看 GitWorkspace 是否满足 §99 目标。
