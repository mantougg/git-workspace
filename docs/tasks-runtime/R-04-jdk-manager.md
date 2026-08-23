# R-04 JDK 检测与 JDK Manager

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。无任务依赖。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | - |
| 对应源文档 | §31 JVM 管理、§32 JDK Manager |

## 目标

自动发现本机全部可用 JDK，建立 JDK 注册表，并支持项目级 JDK 绑定（Project A → JDK 8 / Project B → JDK 17），为 Launcher（R-10）提供可靠的 `java` 可执行路径。

## 需求范围

- [x] JDK 检测：`JAVA_HOME` / `PATH` / 常见安装目录扫描（§31）
- [x] 来源发现：System / mise / jEnv / SDKMAN / Manual（§31）
- [x] 版本与元信息识别：执行 `java -version` 解析 major version（8 / 11 / 17 / 21 / 25+）、vendor、架构、bitness
- [x] JDK 注册表持久化（SQLite 元数据），启动时校验缓存条目仍有效（路径存在 + 版本复核）
- [x] 项目级 JDK 绑定：Runtime 配置可指定 JDK（与 R-07 配置模型对接）
- [x] Manual 添加：用户手选 JDK 根目录，校验通过才入库
- [x] Settings UI：JDK 列表 + 添加/删除/校验

## 架构 / 性能注意点

- `java -version` 输出格式因 vendor 而异（Oracle / OpenJDK / Temurin / GraalVM），解析要宽容并保留原始输出。
- 检测走惰性 + 缓存：注册表命中不重复 fork 进程；失效条目惰性复检，禁止每次启动全量重扫。
- Windows / macOS / Linux 安装目录布局差异（`Contents/Home` 等）要覆盖。
- 检测失败的 JDK 不产生硬错误，标记不可用即可。

## 验收标准

- [x] 本机已安装 JDK 全部被发现，major version / 路径正确
- [x] 手动添加无效路径时给出可行动提示（`JdkNotFound` 类）
- [x] 项目绑定 JDK 后，R-10 启动实际使用该 JDK（联调验证）— 已在 R-10 验证（`runtime::launch::manager::tests::bound_jdk_is_used_for_launch_command`：注册表 upsert `/jdk-21` + 配置 `jdk: "21"`，启动命令预览以 `/jdk-21/bin/java` 开头）
- [x] 缓存条目失效（JDK 被卸载）后能正确标记而非误用

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-23 R-10 启动联调验证通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 R-04：JDK 多来源发现、`java -version` 解析器、SQLite 注册表与惰性校验、Settings UI；项目级绑定对接 R-07、R-10 启动联调在 R-10 验证 |
| 2026-08-19 | ✅ 完成核心实现 | 补齐 IPC 命令层（`commands/jdk.rs`：discover/list/get/add_manual/validate/prune/remove）并注册到 `lib.rs`；修复 `error.rs` 的 `JdkNotFound` code 映射（此前 `code()` 非穷尽导致编译失败）；前端 `types/jdk.ts` + `api/jdk.ts` + `views/JdkManagerView.vue` Settings UI + 路由 + Dashboard 导航入口；IPC golden 快照覆盖 `JdkInstallation`/`JdkDiscoverySource`/`JdkVendor`/`JdkNotFoundError`。验证：`cargo check` 干净、`cargo test java::` 18 passed（含真实 `java -version` 探测）、`cargo test ipc_golden` 2 passed、`vue-tsc --noEmit` 无错。项目级绑定（`runtime_projects.jdk` 列已在 schema）配置 UI 随 R-07、R-10 启动联调在 R-10 验证 |
| 2026-08-23 | ✅ R-10 联调验证 | 「项目绑定 JDK 后，R-10 启动实际使用该 JDK」验收通过：`runtime::launch::manager::tests::bound_jdk_is_used_for_launch_command` 断言启动命令预览以绑定 JDK 的 `/jdk-21/bin/java` 开头（经 R-09 pipeline `resolve_jdk_for_config` → `java_exec_for` 链路） |

### 子任务清单

- [x] 多来源 JDK 发现
- [x] `java -version` 解析器（多 vendor）
- [x] 注册表持久化与惰性校验
- [x] 项目级绑定（对接 R-07）— schema 列就位，配置 UI 随 R-07
- [x] Settings UI
- [x] 单元测试（解析器 + 校验路径）
