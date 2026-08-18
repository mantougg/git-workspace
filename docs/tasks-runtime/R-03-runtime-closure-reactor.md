# R-03 Runtime Closure 与 Synthetic Reactor

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[R-02 Maven 依赖图与 Workspace Source Mapping](./R-02-dependency-graph-source-mapping.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 0 · Runtime 基础设施 |
| 优先级 | P0（前置） |
| 状态 | ✅ 已完成 |
| 依赖 | R-02 |
| 对应源文档 | §14 Runtime Dependency Closure、§15 Runtime Scope、§16 Runtime Reactor、§17 Synthetic Reactor 原则 |

## 目标

给定一个 Spring Boot 应用，沿源码依赖计算**最小 Runtime Closure**，实现「100 个 Repository 也只构建实际运行链路」；跨 repo 场景在 `.gitworkspace/` 生成 Synthetic Runtime Reactor，且绝不修改用户原始 `pom.xml`。

## 需求范围

- [x] Runtime Closure 计算：从目标应用出发，沿 Workspace Source 依赖求传递闭包（§14）；非源码依赖（Local/Remote）不进入构建范围
- [x] Runtime Scope（§15）：`Auto / Manual / Hybrid` 三种模式；Manual 允许手动增删模块，Hybrid 在 Auto 结果上调整
- [x] 标准单仓多模块项目：直接复用 Maven Reactor（构建参数 `-pl <app> -am`，不生成任何文件）
- [x] 跨 Repository：在 `.gitworkspace/runtime/<app>/pom.xml` 生成 Synthetic Reactor（`packaging=pom` + 相对路径 `<modules>`，§16）
- [x] 生成物约束：只写 `.gitworkspace/`，幂等重生成；`.gitworkspace` 默认加入 `.gitignore`（§17）
- [x] Closure / Scope 缓存：纳入 R-02 Graph Cache，POM hash 未变直接复用

## 架构 / 性能注意点

- **用户项目只读**是硬约束（全局约束 §2）：任何路径下都不得改写用户的 pom / 源码。
- Reactor `<module>` 相对路径须处理：Windows 盘符/分隔符、跨盘符无法相对时的报错提示、符号链接。
- 生成的 Synthetic Reactor 必须能被 `mvn -f ... validate` 直接接受——生成逻辑要有真实 mvn 验证的测试（找不到 mvn 时跳过并标注）。
- Closure 算法基于 R-02 缓存图做内存遍历，100 repo 场景毫秒级；变更面 = POM hash 变化的项目子图。
- 环依赖检测：发现循环依赖时给出结构化错误而非死循环。

## 验收标准

- [x] 100 repo 样例 workspace 中运行单应用，Closure 只含实际链路模块（如 boot+web+auth+core+common），其余不参与
- [x] 跨 repo 生成的 Synthetic Reactor 通过 `mvn validate`；单仓多模块场景不生成任何额外文件
- [x] 运行全程用户原始 `pom.xml` 零改动（git status 干净）
- [x] Manual 模式剔除模块后 Closure 正确收缩，Hybrid 模式增删生效
- [x] 循环依赖、缺失模块等异常给出结构化错误

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-18 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-18 | 🟦 开始开发 | 启动 R-03：Runtime Closure、Auto/Manual/Hybrid Scope、Graph fingerprint 缓存、Reactor 直通、Synthetic Reactor 与真实 Maven 验证 |
| 2026-08-18 | ✅ 完成 | 完成源码依赖闭包、三种 Scope、fingerprint 缓存、现有 Reactor 直通、跨仓 Synthetic Reactor、Windows/符号链接路径与 `.gitignore` 安全维护；验证：`cargo test` 216 passed / 2 ignored，`cargo check --all-targets`、`cargo clippy --all-targets --all-features`、`pnpm build` 通过，Maven 3.9.14 真实 `mvn -o validate` 通过，源仓库 git status 保持干净 |

### 子任务清单

- [x] Closure 传递闭包算法（基于 R-02 图）
- [x] Runtime Scope 三模式
- [x] 单仓 Reactor 直通（-pl/-am 参数构造）
- [x] Synthetic Reactor 生成器 + 幂等重写
- [x] `.gitignore` 自动维护
- [x] 单元测试 + 真实 mvn validate 集成测试
