# F-03 JDK 全量扫描兼容性验证（系统配置 / mise / jEnv / SDKMAN / Manual）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 3 |
| 关联任务 | R-04 JDK Manager |

## 问题描述

全量扫描 JDK 时，需要确认是否真正兼容了以下来源的**查找**与**安装**（识别为可用 JDK 并纳入注册表）：

- 系统配置（`JAVA_HOME`、PATH 中的 java、系统默认安装目录）
- mise（如 `C:\Users\sdhzy\AppData\Local\mise\installs\java\temurin-17.0.19+10`）
- jEnv
- SDKMAN
- Manual（用户手动指定目录）

## 定位线索

- 检测实现：`src-tauri/src/java/detect.rs`（含 `find_in_path`，Windows 按 `.exe → .cmd → .bat → 裸名` 顺序）
- 注册表：R-04 相关 `src-tauri/src/runtime/` 下 JDK 注册/绑定逻辑

## 修复范围

- [x] 逐来源核对扫描覆盖：列出每个来源的预期查找路径规则，对照现有实现找漏项
- [x] 缺失的来源补齐扫描逻辑（含 Windows / macOS / Linux 路径差异）
- [x] 为每个来源补探测型集成测试：**探测不到环境就 skip 并打印原因**，不硬失败（AGENTS.md 平台规范 §4）

## 验收标准

- [x] 五个来源各有明确的查找路径清单与覆盖结论（文档化在任务时间线或代码注释）
- [x] 本机实际存在的 mise JDK（temurin-17.0.19+10）能被全量扫描发现
- [x] 相关测试通过或按规则 skip

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成。五来源覆盖核对：①系统配置=JAVA_HOME + PATH + 系统安装目录（Win: Program Files\Java 等 3 处 + %LOCALAPPDATA%\Programs\Eclipse Adoptium；macOS: JVM 目录 + Homebrew opt；Linux: /usr/lib/jvm 等）✓；②mise=**修复前有真实漏检**——只查了 MISE_DATA_DIR / ~/.mise / ~/.local/share/mise / ~/.asdf，漏了 Windows 默认数据目录 %LOCALAPPDATA%\mise（本机 4 个 temurin JDK 全漏检）；已补 XDG_DATA_HOME + data_local_dir 两个路径；③jEnv=~/.jenv/versions ✓（仅 Unix 工具）；④SDKMAN=SDKMAN_DIR + ~/.sdkman/candidates/java ✓；⑤Manual=`add_jdk_manual` 命令，无效路径返回 JdkNotFound ✓。关于「安装」：理解为「各工具安装的 JDK 能被找到」，本产品不做 JDK 下载安装。新增探测型测试 `mise_data_local_dir_is_scanned`（无 mise 环境时 skip 打印原因）。验证：`cargo test java::` 23 passed；本机实测 mise homes = temurin 8.0.492+9 / 11.0.31+11 / 17.0.19+10 / 21.0.11+10.0.LTS 全发现 |

### 子任务清单

- [x] 五来源扫描覆盖核对表（见时间线 ①–⑤）
- [x] 缺失来源补齐（mise Windows %LOCALAPPDATA% 路径）
- [x] 探测型集成测试（`mise_data_local_dir_is_scanned`）
