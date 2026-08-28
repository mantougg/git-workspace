# F-11 Windows 下超长 classpath 导致启动 spawn 失败（os error 206）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 F-04 实测验证时牵出：真实启动 hussar-base-web 构建成功但 spawn 失败 |
| 关联任务 | R-09 Build、R-10 Launcher |

## 问题描述

ClasspathRun 把「模块 target/classes + 全部依赖 jar」拼进 `java -cp <...>` 单条参数。
Windows CreateProcess 命令行上限 32767 字符，企业级项目（hussar-base-web，
数百个依赖 jar 的全路径）轻松超限，启动直接失败：

```
进程 spawn 失败：IO error: 文件名或扩展名太长。 (os error 206)
```

## 修复方案

与 IDEA 的「JAR manifest」缩短策略一致：classpath 估算超阈值（30000 字符）
时，生成只含 `META-INF/MANIFEST.MF` 的 stub jar（manifest `Class-Path` 写全部
条目的 file: URL），启动只传 `-cp pathing.jar`。

- **JDK 8 / 11 / 17 / 21 全兼容**（`Class-Path` 是 JAR 规范基础机制，行为
  各版本一致；`@argfile` 需 JDK 9+，本项目目标含 Java 8 工程，不可用）。
- 产物在 `<workspace>/.gitworkspace/runtime/<name>/classpath/pathing-<hash>.jar`
  （R-14 只读护栏范围内），按 classpath 内容哈希寻址，不变时复用。
- manifest 按 JAR 规范：CRLF、每行 ≤72 字节折行、续行前导空格、目录条目以
  `/` 结尾、空格与非 ASCII 百分号编码（空格不编码会被当成条目分隔符）。

## 验收标准

- [x] 超限 classpath 自动收敛为 pathing jar，不超限零开销
- [x] manifest 折行/编码符合 JAR 规范（单测覆盖）
- [x] JDK 8 / 17 / 21 实测通过
- [x] 真实案例验证：hussar-base-web（release.2）用 pathing jar 成功拉起 JVM

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | 🟦 | F-04 实测牵出：hussar-base-web 真实启动 BUILD SUCCESS 后 spawn os error 206 |
| 2026-08-27 | ✅ | 完成：新增 `runtime/build/pathing_jar.rs`（`estimate_command_len` + `shorten_if_needed` + manifest 构造）+ `strategy.rs::classpath_run_plan` 接入 + Cargo.toml 提升既有传递依赖 `zip 4.6`（default-features=false，Stored 即可）为直接依赖。单测 3 个（72 字节折行/URL 编码与目录斜杠/阈值触发与内容寻址复用）。**真实验证**（代码更新回滚后重新实现并复测）：hussar-base-web BUILD SUCCESS（mvn，68s）→ 以 `-cp ...pathing-<hash>.jar` 拉起 → 120s 宽限到期正常返回 Running（PID 73188），Hussar banner 正常输出，JDK temurin-8 绑定正确，main class 经 R-06 自动推断成功（F-05 链路端到端确认）；应用卡在 banner 之后等外部中间件（dev profile 的 nacos 不可达）——属应用自身环境依赖，非启动管线问题。**多版本验证**：合成用例在 temurin-17.0.19+10 / temurin-21.0.11+10.0.LTS 实测正常加载运行。牵出：stop 未能终止 JVM（另开 F-12） |

### 子任务清单

- [x] pathing jar 生成（manifest 规范 + 内容寻址缓存）
- [x] strategy 接入 + 阈值判断
- [x] 单测 + JDK 8/17/21 实测 + 真实案例验证
