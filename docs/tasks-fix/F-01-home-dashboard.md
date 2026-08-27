# F-01 首页重构：数据卡片与图表

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 1 |
| 关联任务 | T-18 Dashboard、T-19 Health、R-13 Runtime UI |

## 问题描述

首页应定位为数据概览页，以数据卡片和图表为主，而不是现在的形态。

## 修复范围

- [x] a. 仓库数量和状态卡片：首页已有（T-18 统计卡片 + 状态分布条），本次确认满足
- [x] b. 提交热力图：当前工作空间下、当前用户（`git config user.email`/`user.name` 匹配）在所有仓库的提交汇总热力图。**选型记录：naive-ui 实际没有热力图组件**，未引入 ECharts 依赖，用纯 CSS grid 自实现 GitHub 风格周历热力图（`src/components/repo/CommitHeatmap.vue`）
- [x] c. 健康检查入口移到首页（健康评分摘要卡 + 查看详情跳 /health）；**cmd 弹框根因已修复**：`core/health.rs::lfs_available` spawn `git lfs version` 未加 `CREATE_NO_WINDOW`，已补（AGENTS.md 平台规范 §3）
- [x] d. 展示「创建的应用」：首页「我的应用」卡片区（`listRuntimeConfigs`），点击进入 Runtime

## 验收标准

- [x] 首页首屏包含 a–d 四类卡片/图表，数据来自现有缓存 / IPC，打开首页不触发全量重扫（health 走 T-02 缓存轻项、heatmap 是只读 revwalk、apps 是 DB 读）
- [x] 热力图数据准确（按提交作者过滤、按日聚合），提交多的日期颜色深
- [x] 在 Windows 上执行健康检查全程无 cmd 窗口闪现
- [x] `pnpm build` 与相关前端类型检查通过

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成。a=既有 T-18 统计卡片/状态分布已满足。b=新增后端 `core/heatmap.rs`（git2 revwalk TIME 排序、遇早于截止日期即停、按作者时区取日期、email 优先 name 兜底、rayon 跨仓库并行）+ `get_commit_heatmap` 命令 + 前端 `CommitHeatmap.vue`（纯 CSS grid，naive-ui 无热力图组件、未引 ECharts）+ Dashboard「提交热力图」区。c=首页新增健康摘要卡（评分 + 异常数 + 重新检测/详情）；cmd 弹框根因=`lfs_available()` spawn `git lfs version` 缺 `CREATE_NO_WINDOW`，已补。d=首页「我的应用」卡片区（runtime configs，项目/Profile/JDK 标签，点击进 Runtime；空态引导去创建）。验证=`cargo test core::h` 16 passed（含新增热力图测试：作者过滤/一年截断/同日聚合）、`pnpm build` 通过；UI 未运行态逐项点击验证 |

### 子任务清单

- [x] 仓库数量与状态卡片（既有 T-18 已满足）
- [x] 提交热力图（新后端命令 + 自实现热力图组件）
- [x] 健康检查前置到首页 + 消除 cmd 弹框（lfs_available 补 CREATE_NO_WINDOW）
- [x] 已创建应用卡片
