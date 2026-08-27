# F-04 新建应用预设参数与变量（IDEA 启动参数预设）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 新建应用支持「预设模板」机制：预设可包含 JVM 参数、系统属性（-D）、环境变量、javaagent 等
- [ ] 内置至少一套「IDEA 启动」预设，参数对齐上面样例（其中 `idea_rt.jar` 的端口、`@arg_file` 这类 IDEA 私有项需决策：剔除或参数化，决策记录在时间线）
- [ ] 预设中的变量（如 JDK 路径、主类名）支持占位符，创建应用时按实际值替换
- [ ] **实测验证**：用该预设创建并启动一个真实 Spring Boot 应用（可用 `src-tauri/examples/` 或 `golden/` 下的 fixture），进程正常拉起、日志正常输出

## 验收标准

- [ ] 「IDEA 启动」预设创建的应用真实启动成功（时间线记录验证用的应用与 JDK）
- [ ] 预设/变量机制可扩展（后续新增预设不改核心代码，仅加配置）
- [ ] 构建与启动使用同源 JDK（AGENTS.md 平台规范 §4）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：2026-08-27 问题录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |

### 子任务清单

- [ ] 预设模板机制设计
- [ ] 「IDEA 启动」内置预设
- [ ] 变量占位符替换
- [ ] 真实启动实测验证
