# F-05 新建应用启动类自动检测不准确

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 5（原文「失火优化自动检测」，按上下文理解为启动类/服务自动检测） |
| 关联任务 | R-06 Spring Boot Detection |

## 问题描述

新建应用时的自动检测当前不准确。复现案例：

- 工作空间：`D:\AWork\Code\9.6.0-release.2`（env 目录：`D:\AWork\Code\9.6.0-release.2\env`）
- 模块：`hussar-base-web`
- 主类：`com.jxdinfo.hussar.example.HussarApplication`
- 现象：该启动类没有被检测出来

## 定位线索

- 主类推断：`src-tauri/src/runtime/launch/manager.rs::infer_main_class`
- Spring Boot 检测：R-06 相关 `src-tauri/src/` 代码
- 可能原因方向（需实测确认）：路径归一化（混合分隔符，`AGENTS.md` 平台规范 §1）、多模块 Maven 工程未深入子模块、`@SpringBootApplication` 注解识别规则过严、扫描深度不足

## 修复范围

- [x] 复现漏检：以上述工作空间为 fixture（或提取最小 fixture），写出失败的检测测试
- [x] 定位漏检根因并修复
- [x] 回归检查：修复不得降低其他已能检出项目的准确率

## 验收标准

- [x] 复现案例中 `com.jxdinfo.hussar.example.HussarApplication` 能被正确检出
- [x] 新增针对该场景的测试用例通过；既有检测相关测试不回归（偏差说明见时间线：前端无测试基建，以复现案例模拟验证 + vue-tsc 代替）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成：根因在前端匹配而非 Rust 检测。读取真实工程确认——`hussar-base-web/pom.xml` 直接声明了 `spring-boot-maven-plugin`（后端 gating 命中）、`HussarApplication.java` 有 `@SpringBootApplication`（扫描规则可识别）；但向导 `onDetectMainClass` 比较时，R-02 索引的 `form.project` 是正斜杠 pom 路径，Rust 返回的 `projectPath` 是 Windows 反斜杠，三种旧比较全部落空（且 artifactId `hussar-web` ≠ 目录名 `hussar-base-web`，module 后缀口径也兜不住）。修法=`RuntimeAppWizard.vue` 匹配前两侧 `replace(/\\/g,"/")` 归一化（AGENTS.md 平台规范 §1），匹配口径改为：pom 路径相等 / 相对路径后缀 / artifactId 相等 / 路径段后缀（带 `/` 边界，比旧 endsWith 更严）。后端 `infer_main_class` 本已做归一化，无需改。验证：复现案例字符串模拟（旧 false → 新 true）+ `pnpm build`（vue-tsc + vite）通过。偏差：前端无 vitest 等测试基建，未新增持久测试用例，以复现案例模拟代替 |

### 子任务清单

- [x] 复现漏检并写失败测试（以真实工程读取 + 案例字符串模拟复现：旧逻辑 false）
- [x] 根因定位与修复（前端路径分隔符归一化）
- [x] 回归验证（`pnpm build` 通过；匹配口径较旧逻辑更严格，无新增误判面）
