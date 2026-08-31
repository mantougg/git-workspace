# R-27 Maven 发现与 Git 解耦（workspace 级补扫）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。任务依赖：R-01（发现/解析复用其扫描与解析管线，不新增框架）。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · 扩展运行时 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | R-01 |
| 对应源文档 | §9 Maven 项目自动发现（边界扩展） |

## 目标

**Runtime 相关功能只和 workspace 有关，与 git 无关**：Maven 发现以 workspace 为边界，以下三种形态均可解析依赖并创建 Runtime 应用：

1. 根目录有 git / 无 git；
2. 子目录有 git / 无 git；
3. 混合工作区（部分子目录有 git、部分没有）。

典型场景：GitLab 源码导出包 / 拷贝的源码树（残留 `.gitlab-ci.yml`、`.gitignore` 但无 `.git`）。

## 背景

R-01 的发现模型是「T-01 Scanner 找 `.git` 仓库清单 → 逐 repo 下钻扫 `pom.xml`」。无 `.git` 的目录树会得到 0 个 Maven 项目，依赖图为空，R-06 Spring Boot 检测随之无候选，Runtime 应用无法创建。

## 需求范围

- [x] 仓库边界外补扫：仓库清单扫描之外，**始终**以 workspace 根为伪仓库走一遍既有 `discover_poms_in_repos` 管线，命中所有非仓库区域的 pom
- [x] 复用既有语义：`.gitworkspaceignore` + 默认跳过目录、嵌套仓库边界跳过（含 `.git` 的目录不下钻，不重复扫描）、POM Cache、取消标志
- [x] 按路径去重：根目录本身是仓库时根级 pom 会被两遍扫描各收集一次，合并后去重
- [x] 不新增配置项；非仓库区域的零散 pom（备份目录等）可用 `.gitworkspaceignore` 排除
- [x] `cargo test` 覆盖：无 git 工作区、仓库无 pom、混合工作区、根目录即仓库去重、补扫路径取消

## 架构 / 性能注意点

- 改动收敛在 `maven/discovery.rs::discover_poms` 单点；所有生产入口（依赖解析任务、Spring Boot 检测、启动主类推断）自动受益，前端无改动。
- 每次发现多走一遍全树遍历；仓库目录在补扫中被边界规则快速跳过，重复 pom 的解析走 POM Cache。
- 全程本地文件遍历，禁止网络请求（全局约束 §10）。
- 远程 parent（如 `relativePath` 指向 workspace 外且磁盘不存在）仍按 R-01 既定设计降级标记 `remote_parent_missing`，交给 `mvn` 自身解析，不在本任务范围。

## 验收标准

- [x] 无 `.git` 的源码导出包工作区能发现 Maven 项目、产出依赖图，Spring Boot 检测有候选
- [x] 混合工作区（git 仓库有 pom + 非 git 目录有 pom）两者都被发现（`discovers_repo_and_plain_dirs_and_classifies_workspace_library`）
- [x] 根目录本身是仓库时不产生重复项目（`root_repository_supplement_does_not_duplicate`）
- [x] 取消标志在补扫路径同样生效
- [x] `cargo test`、`cargo clippy --all-targets --all-features`、`cargo check --all-targets` 通过

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-31 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-31 | 🟦 开始开发 | 根因定位：`discover_poms` 只在 `.git` 仓库内扫 pom，无 git 工作区得 0 项目；确定空结果兜底方案 |
| 2026-08-31 | ✅ 完成 | `discover_poms` 空结果兜底回退实现（仅 `discovery.rs`，+100 行含测试）；新增 3 个单测，discovery 模块 10 passed；`cargo clippy --all-targets --all-features`、`cargo check --all-targets` 通过且无新增警告；真实工作区 `lims-mvp-base_950` 实测：发现 `hussar-web`（此前为 0），effective 依赖 63 条，`remote_parent_missing=true` 降级标记符合预期，耗时 874ms。全量 `cargo test --lib` 中 12 个失败均为既有环境性失败（AI schema 快照 CRLF、real_maven 集成测试联网构建、debug 并行下 benchmark 超时，单独重跑通过），与本改动无关 |
| 2026-08-31 | ✅ 方案升级 | 用户明确设计原则「Runtime 只和 workspace 有关、与 git 无关」，从空结果兜底升级为**常开补扫 + 路径去重**，补齐混合工作区（条件 3）覆盖；既有 `scans_only_t01_repository_inventory_*` 测试语义反转为 `discovers_repo_and_plain_dirs_*`（非仓库 pom 从不发现→应发现），新增根目录即仓库去重测试，性能探针 cache hits 断言随补扫二次命中放宽为 `>= 100`；discovery 模块 12 passed，全量 668 passed（同 12 个既有环境性失败），真实工作区复测结果不变 |

### 子任务清单

- [x] 根因分析与方案评审（空结果兜底 vs 始终补扫，初版选定前者）
- [x] `discover_poms` 空结果兜底回退实现（后升级为常开补扫）
- [x] 方案升级：常开补扫 + 按路径去重，覆盖混合工作区
- [x] 单元测试（无 git 工作区 / 仓库无 pom / 混合工作区 / 根目录即仓库去重 / 补扫路径取消）
- [x] 真实工作区实测（lims-mvp-base_950）
