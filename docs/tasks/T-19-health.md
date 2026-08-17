# T-19 Workspace Health

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-02 |
| 对应 Roadmap | §19 Workspace Health |

## 目标

实现 Workspace 健康检测与健康评分，自动发现脏/冲突/超前/落后/游离/缺远程/分歧等异常。

## 需求范围

- [x] 检测项：Dirty / Conflict / Ahead / Behind / Detached / Missing Remote / Diverged / Untracked / Large Files / LFS Error / Submodule Error（语义：Dirty=跟踪文件变更；Missing Remote=未配置任何 git remote（`RepoStatus.has_remote` 随状态计算顺带采集）；Diverged=ahead>0 且 behind>0；Large Files=工作区 >10MB 文件（跳过 .git/runtime 目录）；LFS Error=.gitattributes 声明 filter=lfs 但 git-lfs 不可用；Submodule Error=.gitmodules 声明但路径缺失/未初始化）
- [x] 健康评分：0~100%，汇总展示（每仓库 100 起按异常扣权重、下限 0，工作区取四舍五入平均）
- [x] 每项异常可下钻到具体仓库列表（异常卡片/标签点击过滤仓库表）
- [x] 异常仓库排序与快速筛选（评分列可排序默认最差在前、异常类型过滤、仅异常开关、名称搜索）
- [x] 数据基于 T-02 状态缓存，无额外网络/扫描开销（轻项纯 derivation 零 IO）

## 架构 / 性能注意点

- 评分规则可配置（各项权重），放配置文件而非硬编码。
- Large Files / LFS / Submodule 检测属于重项，按需（进入 Health 页时）异步计算，不进常驻状态路径。

## 验收标准

- [x] 各类异常正确识别并可下钻定位仓库（轻项 `anomalies_of` 全组合单测、重项大文件遍历/子模块/gitmodules 解析单测；下钻 = 异常卡片/标签点击过滤仓库表）
- [x] 评分计算规则清晰且可配置（公式文档化；`health-weights.json` 配置文件 serde-default 支持部分覆盖，UI 折叠面板展示当前权重）
- [x] 健康页打开不阻塞 UI（重项异步）（两阶段加载：轻项走 T-02 缓存即时返回；大文件/LFS/子模块独立 command `get_health_extras` 异步合并重算，期间页面可交互）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发 |
| 2026-08-17 | ✅ | 完成：新增 `core/health.rs`（11 项异常检测 `anomalies_of` 纯 derivation + 可配置权重 `health-weights.json`（serde-default 部分覆盖）+ 评分 `score_of`/`aggregate_health` + 重项 `compute_health_extra`：大文件迭代遍历跳过 runtime 目录、LFS 声明+git-lfs 探测（每批一次）、.gitmodules 解析与子模块未初始化检测）+ `commands/health.rs` 两命令（`get_workspace_health` 走 T-02 缓存轻项即时返回、`get_health_extras` rayon 并行重项）；`RepoStatus` 补 `has_remote`（Missing Remote 信号，status 计算顺带采集）+ IPC golden/TS 同步；前端 `HealthView.vue`（评分面板 + 权重折叠 + 11 异常卡片点击下钻过滤 + 仓库表排序/仅异常/搜索，重项异步合并按同公式重算分）+ `/health` 路由 + Dashboard 健康检查入口；10 个 health 单测；`cargo test` 124 passed、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] 检测项实现与评分规则
- [x] Health 页 UI（评分 + 异常下钻）
- [x] 异常筛选联动
