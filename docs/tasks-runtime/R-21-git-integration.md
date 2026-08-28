# R-21 Git 联动（Status 提示 / Branch 联动 / 操作保护）

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md) 与 `../tasks/00-全局开发约束.md`（Git 侧约束）；直接依赖：[R-10 Runtime Launcher 与 Process Manager](./R-10-launcher-process-manager.md)、T-02 Status Engine、T-09 Branch Manager。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · 多服务与效率 |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | R-10, T-02, T-09 |
| 对应源文档 | §47 Runtime 与 GitWorkspace 联动、§48 Branch 联动、§49 Git Operation 安全 |

## 目标

打通 Runtime 与 Git Workspace：依赖模块的 Git 状态变化时提示重建，切换分支时自动失效依赖模型并按需重建，运行期间的危险 Git 操作有护栏。

## 需求范围

- [x] Status 联动（§47）：参与运行中应用 Closure 的仓库出现 Modified 时，Runtime 提示 `Runtime Dependency Changed`（列出受影响模块）+ [Rebuild & Restart] 入口
- [x] Branch 联动（§48）：分支切换后自动 Invalidate Dependency Model → Recalculate Maven Graph → Check POM Changes → Rebuild if required
- [x] Git 操作保护（§49）：有应用 Running 时执行 Checkout / 切分支，弹出确认（`Stop & Switch / Cancel`），说明运行中应用与风险
- [x] Runtime 侧**不主动修改任何 Git 状态**（全局约束 §11）；本任务只做监听、提示与拦截确认
- [x] 提示可稍后处理（snooze），不强制打断用户

## 架构 / 性能注意点

- 数据源用 T-02 状态缓存与事件订阅，**禁止为联动触发额外 git 操作或网络请求**。
- 分支切换后的模型重算走 R-02/R-03 既有缓存失效路径，不另建通道。
- 拦截确认接入 Git 侧操作入口（T-09/T-13 等），以查询「是否有运行中应用」的轻量 IPC 实现，不得拖慢正常 Git 操作。
- 联动提示聚合适度节流，批量仓库变化合并为一条提示。

## 验收标准

- [x] 修改被依赖模块（auth/common）后 Runtime 出现受影响提示，一键 Rebuild & Restart 生效
- [x] 切换分支后依赖模型正确失效与重算，POM 有变化时才触发 Rebuild
- [x] 运行中切分支被拦截并给出 Stop & Switch / Cancel 选择
- [x] 无运行中应用时 Git 操作零额外开销

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-29

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | ✅ | 完成：`GitLinkEngine`（runtime/git_link.rs）——§47 Status 联动每 5s 只读 `repo_status` 快照（T-02 缓存，禁 git 调用），闭包模块按 `repository_id` 关联 dirty 仓库 → 聚合一条 `runtime_dependency_changed` 提示（快照去重，恢复干净发空提示清除）；§48 Branch 联动接入 `checkout_branch` / `batch_branch_op` 成功路径 → 记录 pre-fingerprint + 提交 `ResolveDependencies`（R-02 既有失效重算路径）→ 周期对账比对 fingerprint，**POM 有变化才**发提示并自动 Rebuild & Restart（仅 autoRestart 应用；含 15s 重试窗口覆盖重算先于切换的竞态）；§49 保护：`runtime_running_briefs` 轻量 DB 读 IPC + 前端 `guardRuntimeRunning`（BranchManager 单仓 / RepositoryList 批量检出入口，`Stop & Switch / Cancel`，Stop 走 `runtime_stop_blocking` 同步优雅停止），无运行中应用零额外开销。前端：Dashboard 提示条（Rebuild & Restart / 稍后 snooze）。测试：git_link 3 项集成测试（真 DB 索引 + 假提交端：聚合去重、POM 变化才重建、无变化不重建）+ golden 快照（DependencyChangedPayload / RuntimeRunningBrief）+ TS 类型对照。验证：`cargo test --lib` 487 通过（5 个失败均为既有环境/负载敏感问题，单跑通过）、`vue-tsc` + `vite build` 通过 |

### 子任务清单

- [x] Closure 仓库 Git 状态订阅与提示
- [x] Branch 切换 → 模型失效/重算链路
- [x] Git 操作拦截确认
- [x] UI（提示条 / 确认框）
- [x] 单元/集成测试
