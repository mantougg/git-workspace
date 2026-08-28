# F-04 新建应用预设参数与变量（IDEA 启动参数预设）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 4 |
| 关联任务 | R-07 Runtime Config、R-10 Launcher、R-19 Runtime Templates |

## 问题描述

新建应用时希望可以预设一些参数和变量。典型场景：模拟 IDEA 启动 Spring Boot 应用时的完整 JVM 参数，可基于「IDEA 启动」预设出一套模板。

用户提供的 IDEA 实际启动命令（基准样例，需验证按此预设真正可以跑起来）：

```
C:\Users\sdhzy\AppData\Local\mise\installs\java\temurin-17.0.19+10\bin\java.exe
  -XX:TieredStopAtLevel=1
  -Dspring.output.ansi.enabled=always
  -Dcom.sun.management.jmxremote
  -Dspring.jmx.enabled=true
  -Dspring.liveBeansView.mbeanDomain
  -Dspring.application.admin.enabled=true
  "-Dmanagement.endpoints.jmx.exposure.include=*"
  "-javaagent:D:\Program Files\JetBrains\IntelliJ IDEA 2026.2.0.1\lib\idea_rt.jar=59671"
  -Dfile.encoding=UTF-8
  @C:\Users\sdhzy\AppData\Local\Temp\idea_arg_file339476742
  com.jxdinfo.hussar.example.HussarApplication
```

## 修复范围

- [x] 新建应用支持「预设模板」机制：预设可包含 JVM 参数、系统属性（-D）、环境变量、javaagent 等（实现为预设 = VM Options 清单，见时间线决策）
- [x] 内置至少一套「IDEA 启动」预设，参数对齐上面样例（其中 `idea_rt.jar` 的端口、`@arg_file` 这类 IDEA 私有项需决策：剔除或参数化，决策记录在时间线）
- [x] 预设中的变量（如 JDK 路径、主类名）支持占位符，创建应用时按实际值替换（见时间线决策：JDK 由 JDK 绑定字段承担、主类由 Main Class 字段承担，无需占位符）
- [x] **实测验证**：用该预设创建并启动一个真实 Spring Boot 应用（可用 `src-tauri/examples/` 或 `golden/` 下的 fixture），进程正常拉起、日志正常输出

## 验收标准

- [x] 「IDEA 启动」预设创建的应用真实启动成功（时间线记录验证用的应用与 JDK）
- [x] 预设/变量机制可扩展（后续新增预设不改核心代码，仅加配置）
- [x] 构建与启动使用同源 JDK（AGENTS.md 平台规范 §4）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成。实现：新增 `src/config/launchPresets.ts`（预设清单配置化，新增预设只加数组条目），内置「IDEA 启动（Spring Boot）」预设 = 样例命令中的 8 个 JVM 参数；向导「VM Options」上方新增「启动预设」下拉，选择即覆盖填充 + 展示预设说明。**决策记录**：①`-javaagent:idea_rt.jar=<port>`（IDEA 调试器代理，无 IDEA 监听无意义且路径随本机安装变）与 `@idea_arg_file*`（IDEA 动态生成）属 IDEA 私有项，**剔除**；②变量占位符未做——JDK 由 JDK 绑定字段、主类由 Main Class 字段承担（与运行时配置模型一致），用户语境的「变量」即环境变量，向导已有环境变量编辑区。**实测验证**：复用 R-10 真实 Spring Boot fixture（boot_fixture 加 vm_options 参数），新增集成测试 `idea_preset_vm_options_boot_real_spring_boot_app`——全套预设参数下真实起到 Running 再正常停止，通过（JDK temurin-17）。**注意**：这类 real-maven 集成测试要求 JAVA_HOME≥17，本机默认 JAVA_HOME=temurin-8 时既有测试（classpath_run_full_cycle 等）同样失败——属既有环境问题，非本次引入（验证命令：`JAVA_HOME=...temurin-17... cargo test idea_preset`；`pnpm build` 通过）。注：本次实现曾因代码更新被回滚，此为重新实现（内容一致，适配了 Desktop Skin 后的 tokens 与向导结构） |

### 子任务清单

- [x] 预设模板机制设计（`src/config/launchPresets.ts` 配置化）
- [x] 「IDEA 启动」内置预设（剔除 idea_rt.jar / @arg_file）
- [x] 变量占位符替换（决策：不做占位符，JDK/主类由配置字段承担，环境变量走既有编辑区）
- [x] 真实启动实测验证（集成测试起到 Running 再停止，通过）
