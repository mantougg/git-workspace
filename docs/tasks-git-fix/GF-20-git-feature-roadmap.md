# GF-20 git 功能增强规划待排期（Blame / Interactive Rebase / Remote 管理 / Binary diff 等）

> 状态：💬 仅讨论（不排期）
> 优先级：—
> 来源：2026-09-24 Git 使用体验全景盘点（feature-proposals + Roadmap 未落地项对账）。

## 议题描述

以下 git 功能增强在提案/Roadmap 中已规划但未落地。本档案登记候选与对账状态，
**不进入修复流程**；每项待用户拍板优先级后再拆正式 GF-XX 任务。

## 候选清单（含对账）

| 功能 | 提案评级/工作量 | 依据 | 对账状态 |
|---|---|---|---|
| Blame 视图（逐行最后修改者，与 diff 联动） | ⭐⭐⭐⭐ / 中 | feature-proposals:10-26、:487-495 | 未落地 |
| Interactive Rebase UI（拖拽排序、pick/squash/reword/drop/edit，冲突复用 ConflictResolver） | ⭐⭐⭐ / 大 | feature-proposals:68-86、:507-514 | 未落地；注意 RebaseDialog.vue:28-30 已有 HTML5 拖拽先例 |
| Remote 仓库管理界面（列表/增删改/fork 关系可视化） | ⭐⭐⭐ / 小 | feature-proposals:131-147、:495 | 未落地（当前 detect_remote/PR/CI 已有，缺 remote CRUD 界面） |
| Git 统计面板扩展（贡献者/活跃度/分支生命周期） | ⭐⭐⭐ / 中 | feature-proposals:291-311、:501 | 部分已有基础：`get_commit_heatmap`（heatmap.rs:12）已服务 DashboardView |
| Binary Diff（PNG/JPG/SVG/PDF before/after；JSON/XML/YAML 格式化 diff） | P2 | Roadmap:1085-1112 | 未落地 |
| Git 配置读写 UI（user.name/email/core.autocrlf/credential.helper/pull.rebase，危险配置提示） | P2 | Roadmap:1732-1752 | 未落地 |
| 主题三档切换 UI（亮/暗/跟随系统） | — | project-analysis:115 | `setMode` 已实现（useTheme.ts:43）但**零调用方**，缺入口 |
| 触屏/窄屏适配（git 视图 0 处 @media） | — | 2026-09-24 盘点 | 桌面 IDE 工具定位下低优先；确认后可能不做 |
| Submodule 批量更新 / 父子树可视化 | ⭐⭐⭐ / 中 | feature-proposals:29-46、:513 | 部分已落地：T-30（submodule/LFS/hooks 基础操作 ✅，tasks/README:87），缺批量与可视化 |

## 已对账排除（提案已过时）

- PR/MR 集成：T-29 已落地（BranchManager Create PR + CI 状态，tasks/README:86）。
- 命令面板含 git 操作：部分落地（registry.ts:115-206），剩余缺口转 GF-18。
- Git Hooks 可视化管理：T-30 已落地（RepoToolsView）。

## 讨论结论区

（待用户讨论后填写：哪些转正式任务 / 转 GF-XX 编号 / 确认不做）

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建档案：提案与 Roadmap 对账完成，9 项候选（3 项部分已有基础，3 项已排除）。待排期讨论。 |
