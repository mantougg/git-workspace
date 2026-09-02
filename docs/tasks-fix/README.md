# GitWorkspace 问题修复任务总览

> 来源：2026-08-27 用户实测反馈的问题清单（10 项）；2026-08-28 追加 3 项（F-17~F-19）；2026-08-28 下午场追加 3 项（F-20~F-22）；2026-09-02 追加 7 项（F-25~F-31）。
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

- 任务总数：**31**（全部已转为正式任务）
- 已完成：**30** · 修复中：**1** · 未开始：**0** · 仅讨论：**0**

---

## 任务索引

| 编号 | 问题 | 优先级 | 状态 | 文档 |
|---|---|---|---|---|
| F-01 | 首页重构：数据卡片与图表（仓库卡片 / 提交热力图 / 健康检查无 cmd 弹框 / 已创建应用） | P0 | ✅ | [F-01-home-dashboard.md](./F-01-home-dashboard.md) |
| F-02 | Change Set 页面无法返回首页 | P0 | ✅ | [F-02-changeset-navigation.md](./F-02-changeset-navigation.md) |
| F-03 | JDK 全量扫描兼容性验证（系统配置 / mise / jEnv / SDKMAN / Manual） | P1 | ✅ | [F-03-jdk-scan-compat.md](./F-03-jdk-scan-compat.md) |
| F-04 | 新建应用预设参数与变量（IDEA 启动参数预设，需实测跑通） | P1 | ✅ | [F-04-app-launch-presets.md](./F-04-app-launch-presets.md) |
| F-05 | 新建应用启动类自动检测不准确（hussar-base-web HussarApplication 漏检） | P0 | ✅ | [F-05-main-class-detection.md](./F-05-main-class-detection.md) |
| F-06 | 打包启动后任务栏不显示应用图标 | P1 | ✅ | [F-06-taskbar-icon.md](./F-06-taskbar-icon.md) |
| F-07 | 应用底部增加版本与作者栏（vX.Y.Z by author），规则写入 AGENTS.md | P2 | ✅ | [F-07-footer-version-bar.md](./F-07-footer-version-bar.md) |
| F-08 | 工作区管理页面（卡片：名称 / 目录路径 / 扫描深度） | P1 | ✅ | [F-08-workspace-management.md](./F-08-workspace-management.md) |
| F-09 | 变更与操作页 Git 树问题集合（8 个子项） | P0 | ✅ | [F-09-git-tree-ux.md](./F-09-git-tree-ux.md) |
| F-10 | UI 客户端化：Desktop Skin + IDEA 式布局骨架（[改造方案](../desktop-skin-plan.md)） | P3 | ✅ | [F-10-native-ui-discussion.md](./F-10-native-ui-discussion.md) |
| F-11 | Windows 超长 classpath 启动 spawn 失败（os error 206）→ pathing jar（JDK 8/17/21 兼容） | P0 | ✅ | [F-11-classpath-too-long.md](./F-11-classpath-too-long.md) |
| F-12 | Stop 无法终止已启动的 JVM（Windows） | P0 | ✅ | [F-12-stop-jvm-leak.md](./F-12-stop-jvm-leak.md) |
| F-13 | 内容区不可见（AppShell 列向布局把内容区挤到 0 高，只剩左侧菜单） | P0 | ✅ | [F-13-shell-content-zero-height.md](./F-13-shell-content-zero-height.md) |
| F-14 | Git 视图「未指定仓库路径」（当前仓库全局状态缺失，SideNav 直达全灭） | P0 | ✅ | [F-14-git-views-current-repo.md](./F-14-git-views-current-repo.md) |
| F-15 | Runtime 分组无数据 + 无「新建应用」入口（事件名含 `.` 使 listen 抛错阻断加载；子视图 workspaceId 依赖总览写入） | P0 | ✅ | [F-15-runtime-no-data-no-create.md](./F-15-runtime-no-data-no-create.md) |
| F-16 | Maven 可执行体扫描/手动添加 + 本地仓库路径可选 | P1 | ✅ | [F-16-maven-scan-and-local-repo.md](./F-16-maven-scan-and-local-repo.md) |
| F-17 | Git 视图「未指定仓库路径」复现（当前仓库缺少工作区兜底） | P0 | ✅ | [F-17-git-views-repo-auto-fallback.md](./F-17-git-views-repo-auto-fallback.md) |
| F-18 | Change Set 页空状态未占满（n-spin 容器不参与 flex 布局） | P1 | ✅ | [F-18-changeset-empty-layout.md](./F-18-changeset-empty-layout.md) |
| F-19 | 总览热力图未横向占满且悬浮无提示 | P1 | 🟦 | [F-19-heatmap-width-tooltip.md](./F-19-heatmap-width-tooltip.md) |
| F-20 | 变更页 graph/diff 分隔条无法拖拽 + 双空状态 + 空状态不居中 | P1 | ✅ | [F-20-changes-splitter-empty-state.md](./F-20-changes-splitter-empty-state.md) |
| F-21 | 概览热力图缺横纵坐标 + 摘要行未跟踪未水平对齐 | P1 | ✅ | [F-21-dashboard-heatmap-axis-alignment.md](./F-21-dashboard-heatmap-axis-alignment.md) |
| F-22 | Git 视图（提交图/分支/Stash/Worktree/Reflog）支持切换仓库 | P1 | ✅ | [F-22-git-views-repo-switcher.md](./F-22-git-views-repo-switcher.md) |
| F-23 | Runtime 总览加「启动方式」列（直接启动 vs 源码启动 ×n） | P1 | ✅ | [F-23-runtime-dashboard-launch-mode.md](./F-23-runtime-dashboard-launch-mode.md) |
| F-24 | 新建应用切「前端工程」应用永久无响应（RuntimeService 自死锁） | P0 | ✅ | [F-24-wizard-node-unified-project-deadlock.md](./F-24-wizard-node-unified-project-deadlock.md) |
| F-25 | 端口检测不准确：有端口仍被占用却判定空闲 | P1 | ✅ | [F-25-port-detection-accuracy.md](./F-25-port-detection-accuracy.md) |
| F-26 | Node 前端项目启动多端口展示/停止不完整 | P1 | ✅ | [F-26-node-multi-port-runtime.md](./F-26-node-multi-port-runtime.md) |
| F-27 | Node 工具链扫描闪窗且包管理器探测失败 | P1 | ✅ | [F-27-node-toolchain-scan-window.md](./F-27-node-toolchain-scan-window.md) |
| F-28 | 启动时恢复上次选中的工作区 | P1 | ✅ | [F-28-workspace-restore-last-selection.md](./F-28-workspace-restore-last-selection.md) |
| F-29 | Runtime 总览脚本确认区默认折叠并提示更新 | P2 | ✅ | [F-29-runtime-script-approvals-collapsed.md](./F-29-runtime-script-approvals-collapsed.md) |
| F-30 | StatusBar 工作区右侧 watcher 点接入真实状态 | P2 | ✅ | [F-30-statusbar-watcher-state.md](./F-30-statusbar-watcher-state.md) |
| F-31 | Runtime 总览 Applications 删除项危险样式强化 | P2 | ✅ | [F-31-runtime-delete-action-danger-style.md](./F-31-runtime-delete-action-danger-style.md) |

---

## 维护规范

1. 修复任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成修复需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 结果 + 验证方式）。
3. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因；💬 讨论项不参与状态流转，结论明确后可转为正式任务（重新编号或复用原编号置 ⬜）。
4. 一个修复任务尽量一次完成；若牵出独立新问题，新增 F-XX 文档并同步本表，不要在原文档里无限扩张范围。
5. 修复中涉及平台差异（路径 / 进程 / 可执行文件检测）时，先对照根目录 `AGENTS.md` 的「平台兼容性开发规范」。
