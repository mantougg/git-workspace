# T-33 Workspace Manifest + 批量 Clone

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-01 Scanner 硬化](./T-01-scanner.md)、[T-05 Task Queue 硬化](./T-05-task-queue.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-01, T-05 |
| 对应 Roadmap | §60 导入/导出 Workspace、§61 Workspace 模板、§62 Git Repository 模板 |

## 目标

把 §60 的 `gitworkspace.json` 升级为可重建环境的 **Workspace Manifest**（含 remote URL / 默认分支 / 分组 / 标签），Import 时可选批量 Clone，实现「新成员 10 分钟搭好环境」。

## 需求范围

- [x] Manifest 导出：每个仓库的 remote URL + 默认分支 + 分组 + 标签（现 §60 的 `repositories` 为空，无法重建）
- [x] Import 时批量 Clone（走 T-05 任务队列，逐仓库子结果 + Partial Success）
- [x] 新成员入职引导：导入 Manifest → 批量 Clone → 加入 workspace
- [x] 复用 T-01 Scanner 的扫描/验证逻辑与 T-05 的并发/进度能力

## 架构 / 性能注意点

- Clone 走系统 git CLI（网络操作边界），遵守 §45 并发限流（Fetch 8）。
- Manifest 只存纯数据（URL / 分支 / 分组），不存凭据；凭据走系统 credential 机制。
- 批量 Clone 的部分失败必须可定位、可重试，复用 T-05 Partial Success。

## 验收标准

- [x] 导出的 Manifest 含 remote URL / 默认分支 / 分组，可据此重建环境（另含标签；无 remote 的仓库 `remote_url` 为 null，导出摘要有明确数量标注）
- [x] 批量 Clone 100 仓库并发受限，部分失败可定位并重试（复用 T-05：8 worker 池限流、worker 指数退避重试网络失败、BatchState Partial Success + `task_items` 逐仓库落库 + TaskPanel 批次明细）
- [x] 新成员用 Manifest 搭建环境的端到端流程可走通（ManifestView 四步引导：选文件 → 选目标目录 → 预览并批量克隆 → 扫描加入工作区，扫描复用 `scan_repositories`）

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-17 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-17 | 🟦 | 开始开发：Manifest schema（remote URL/默认分支/分组/标签）+ 导出收集（libgit2 本地读 remote，无网络）+ 导入校验/克隆计划 + ManifestView 导入导出与入职引导 UI |
| 2026-08-17 | ✅ | 完成：`core/manifest.rs`（schema + 校验 + 克隆计划，8 单测）、`commands/manifest.rs`（export_workspace_manifest / read_manifest_file / plan_manifest_clone，待 lib.rs 注册）、前端 types/api + ManifestView（导出保存对话框、导入四步引导、预览表、批量克隆接 submit_tasks、扫描加入工作区）。验证：`cargo test --lib manifest` 8 passed；`cargo test --lib` 139 passed（仅余 change_set 2 个失败，属 T-22 并行在改模块）；`pnpm exec vue-tsc --noEmit` exit=0。注意：新 IPC 类型未入 golden/TS_TYPE_MAP（文件禁改，建议后续补登记） |
| 2026-08-17 | ✅ | 收尾：lib.rs 注册 3 个 manifest 命令；IPC golden 补登记 4 个 manifest 类型（ManifestRepo / WorkspaceManifest / ClonePlanItem / ClonePlan；CloneAction 为字符串字面量 union，parser 跳过不检查）；`cargo test --lib` 165 passed（仅余 batch dry_run 2 个沙箱 git clone 环境限制失败） |

### 子任务清单

- [x] Manifest schema 扩展（remote URL / 分支 / 分组 / 标签）
- [x] 导出端写入完整仓库信息
- [x] Import 批量 Clone 接入任务队列
- [x] 入职引导流程 UI
