# GitWorkspace 问题修复任务总览

> 来源：2026-08-27 用户实测反馈的问题清单（10 项）。
> 拆分原则：**每个问题一个独立文档**（同目录下 `F-XX-<slug>.md`），可独立跟踪修复进度与验收。
> 本文件是唯一的修复进度索引；每个任务文档内另有自己的「进度」章节。
>
> 横切约束不重复记录：Git 功能相关遵守 [docs/tasks/00-全局开发约束.md](../tasks/00-全局开发约束.md)，Runtime 相关遵守 [docs/tasks-runtime/00-全局开发约束.md](../tasks-runtime/00-全局开发约束.md)，平台兼容性遵守根目录 `AGENTS.md` 的「平台兼容性开发规范」。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 修复中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |
| 💬 | 仅讨论（不排期） |

## 总体进度

- 任务总数：**10**（含 1 个讨论项）
- 已完成：**8** · 修复中：**0** · 未开始：**1** · 仅讨论：**1**

---

## 任务索引

| 编号 | 问题 | 优先级 | 状态 | 文档 |
|---|---|---|---|---|
| F-01 | 首页重构：数据卡片与图表（仓库卡片 / 提交热力图 / 健康检查无 cmd 弹框 / 已创建应用） | P0 | ✅ | [F-01-home-dashboard.md](./F-01-home-dashboard.md) |
| F-02 | Change Set 页面无法返回首页 | P0 | ✅ | [F-02-changeset-navigation.md](./F-02-changeset-navigation.md) |
| F-03 | JDK 全量扫描兼容性验证（系统配置 / mise / jEnv / SDKMAN / Manual） | P1 | ✅ | [F-03-jdk-scan-compat.md](./F-03-jdk-scan-compat.md) |
| F-04 | 新建应用预设参数与变量（IDEA 启动参数预设，需实测跑通） | P1 | ⬜ | [F-04-app-launch-presets.md](./F-04-app-launch-presets.md) |
| F-05 | 新建应用启动类自动检测不准确（hussar-base-web HussarApplication 漏检） | P0 | ✅ | [F-05-main-class-detection.md](./F-05-main-class-detection.md) |
| F-06 | 打包启动后任务栏不显示应用图标 | P1 | ✅ | [F-06-taskbar-icon.md](./F-06-taskbar-icon.md) |
| F-07 | 应用底部增加版本与作者栏（vX.Y.Z by author），规则写入 AGENTS.md | P2 | ✅ | [F-07-footer-version-bar.md](./F-07-footer-version-bar.md) |
| F-08 | 工作区管理页面（卡片：名称 / 目录路径 / 扫描深度） | P1 | ✅ | [F-08-workspace-management.md](./F-08-workspace-management.md) |
| F-09 | 变更与操作页 Git 树问题集合（8 个子项） | P0 | ✅ | [F-09-git-tree-ux.md](./F-09-git-tree-ux.md) |
| F-10 | UI 客户端化讨论（不像 Web 套壳） | P3 | 💬 | [F-10-native-ui-discussion.md](./F-10-native-ui-discussion.md) |

---

## 维护规范

1. 修复任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成修复需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 结果 + 验证方式）。
3. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因；💬 讨论项不参与状态流转，结论明确后可转为正式任务（重新编号或复用原编号置 ⬜）。
4. 一个修复任务尽量一次完成；若牵出独立新问题，新增 F-XX 文档并同步本表，不要在原文档里无限扩张范围。
5. 修复中涉及平台差异（路径 / 进程 / 可执行文件检测）时，先对照根目录 `AGENTS.md` 的「平台兼容性开发规范」。
