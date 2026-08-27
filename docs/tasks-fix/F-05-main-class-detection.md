# F-05 新建应用启动类自动检测不准确

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
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

- [ ] 复现漏检：以上述工作空间为 fixture（或提取最小 fixture），写出失败的检测测试
- [ ] 定位漏检根因并修复
- [ ] 回归检查：修复不得降低其他已能检出项目的准确率

## 验收标准

- [ ] 复现案例中 `com.jxdinfo.hussar.example.HussarApplication` 能被正确检出
- [ ] 新增针对该场景的测试用例通过；既有检测相关测试不回归

## 进度

### 状态

- 当前状态：未开始
- 最近更新：2026-08-27 问题录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |

### 子任务清单

- [ ] 复现漏检并写失败测试
- [ ] 根因定位与修复
- [ ] 回归验证
