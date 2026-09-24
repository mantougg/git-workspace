---
name: gitworkspace-git-fix
description: GitWorkspace Git 功能体验修复批次（docs/tasks-git-fix/ GF-01~GF-20，源自 2026-09-24 Git 使用体验静态盘点）的执行流程：任务选择、静态证据复现、修复实施、双处进度同步。当用户点名 GF-XX 任务，或说「git 体验修复批次 / 搜索框 / 批量反馈 / Tags / 分叉跟进」等 GF 清单条目时使用；用户运行时反馈的通用快修走 gitworkspace-fix，分析报告批次走 gitworkspace-project-analysis-fix。
---

# GitWorkspace Git 功能体验修复批次（GF-01~GF-20）

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-git-fix/` 的任务文档
**开始**或**继续**一个 Git 使用体验修复任务（GF-XX 编号），并在完成后同步进度。

## 与其他任务系列的边界（先分清）

| 场景 | 系列 / skill |
|---|---|
| git 功能使用体验系统性改进，源于 2026-09-24 静态盘点 | **GF-XX（本 skill）** |
| 用户运行时实测反馈的通用快修（终端、AI、更新器等） | F-XX / gitworkspace-fix |
| 2026-09-13 项目分析报告批次 | PAF-XX / gitworkspace-project-analysis-fix |
| git 新功能规划（Blame / Interactive Rebase 等） | GF-20 💬 档案，**不进修复流程**，待用户排期后拆新 GF-XX |

## 文档地图

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/tasks-git-fix/README.md` | 总索引：GF-01~GF-20 优先级/状态总表 + 维护规范 | 选任务、核对状态 |
| `docs/tasks-git-fix/GF-XX-*.md` | 任务 spec：问题描述 / 定位线索（file:line 证据）/ 修复范围 checklist / 不做 / 验收标准 / 进度 | 修复目标问题时 |

约束文档（按改动范围加载，不重复读）：

| 改动涉及 | 先读 |
|---|---|
| 任何 git 功能逻辑 | `docs/tasks/00-全局开发约束.md`（四原则：Multi-Repo First / Safety First 等） |
| 路径 / 进程 / 可执行文件检测 / 网络命令 | 根目录 `AGENTS.md`「平台兼容性开发规范」 |
| 后端 Tauri 命令线程模型（GF-07 等异步化任务必读） | 根目录 `AGENTS.md`「Tauri 命令线程模型硬规则（F-43）」 |
| 前端 UI / 组件 | `docs/desktop-skin-plan.md` + 根目录 `AGENTS.md`「Desktop Skin 约定」（naive-ui 强制、tokens 变量） |
| 危险操作交互（GF-04/15/16/17） | `docs/GitWorkspace 产品需求与技术架构 Roadmap.md` §46 危险三级 |

## 开始一个任务

1. 确定编号：用户点名 GF-XX，或从 README 总表按优先级选 ⬜（GF-20 是 💬 讨论档案，
   不进入本流程）。**优先看「任务间依赖与建议顺序」一节**，避免逆序返工。
2. 读任务文档全文，明确：问题描述、定位线索、修复范围 checklist、「不做」边界、验收标准。
3. **复现优先（本批次与 F-XX 的关键差异）**：GF 任务的定位线索全部是**静态盘点证据**
   （file:line），不是运行时反馈——动手前必须逐条核对：
   - file:line 是否仍成立（代码演进会漂移）；
   - 问题是否仍存在（**已有先例：GF-13 原 a 子项经核验已被 PAF-26 修复，撤销**）；
   - 能写失败测试的先写失败测试（后端逻辑类任务尽量补测试）。
   核验结论（成立 / 漂移后仍成立 / 已不存在）写回任务文档「定位线索」或「进度」。
4. 状态 `⬜ → 🟦`：**两处同步**（README 总表 + 任务文档「进度」），时间线追加一行
   「开始修复（含复现结论）」。
5. **改 Rust 符号前跑 impact**（AGENTS.md 强制）：`impact({target: "<符号>", direction:
   "upstream"})`，HIGH/CRITICAL 风险先向用户警告再动手。

## 继续一个任务（恢复进行中的）

1. 读任务文档「进度」时间线**最后一条** + checklist 勾选情况。
2. 对照 README 总表该行核对状态一致（不一致以任务文档为准，并修正 README）。
3. 从时间线记录恢复上下文，继续未勾选子任务。

## 完成一个任务

1. 逐条核对「验收标准」，**全部满足**才算完成；有复现案例的用原案例实测验证。
2. 运行验证（按改动范围选择）：
   - 前端：`pnpm build`（含 vue-tsc 类型检查）；
   - 后端：Windows 必须 `GW_TEST_MANIFEST=1 cargo test --lib`（原因见 AGENTS.md 平台规范）；
   - 平台差异改动：`#[cfg(windows)]` / `#[cfg(not(windows))]` 分支两侧都过一遍。
3. **提交前跑 `detect_changes()`**（AGENTS.md 强制），确认只影响预期符号与执行流。
4. 更新任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 根因 + 修法 + 验证命令）。
5. 同步 README 总表该行状态。
6. 牵出独立新问题：新增 GF-XX 文档 + README 总表加行，不在原任务里扩张范围。

## 任务间依赖与建议顺序

**硬依赖**：
- GF-14（死代码清理）依赖 GF-07 定稿——`sync_fetch/pull/push` 死 wrapper 是删是接线
  由 GF-07 的方案决定，GF-14 必须后做。

**建议顺序（失败反馈链）**：GF-07（单仓操作流式化）→ GF-08（认证错误分类）→
GF-02（批量失败前端汇总）。三者独立可修，但按此序可避免 GF-02 的汇总 UI 被 GF-08
的结构化错误返工一遍。

**可连做**：GF-15（批量分叉跟进）与 GF-17（破坏性操作预演）共享 `batch_dry_run`
预演设施，后端 preview 命令可一并设计。

**优先提级候选**：GF-13c（`pick_continue` 丢原作者，`core/history.rs:291` `&sig, &sig`）
是提交数据正确性问题而非纯体验问题，团队仓库建议优先修。

## 必须遵守

- **两处同步**：README 总表与任务文档「进度」必须同时更新，规则以
  `docs/tasks-git-fix/README.md` 末尾「维护规范」为准。
- **静态证据先复现再修**：不复现直接改 = 可能修一个已不存在的问题（GF-13a 教训）。
- **最小修复**：只修文档「修复范围」内的问题，不顺手重构；范围外发现的新问题另立 GF-XX。
- **平台规范**：路径归一化（`replace('\\', "/")`）、PATHEXT 扩展名候选、`CREATE_NO_WINDOW`、
  `.cmd` 经 `cmd /C` 等，见 AGENTS.md；同步 Tauri 命令内禁裸 `tokio::spawn`（F-43）。
- **UI 规范**：交互控件一律 naive-ui（GF 视图内的原生控件正是 GF-05 要清的债，新代码
  不得再引入）；样式用 `--gw-*` tokens；快捷键只走命令注册表。
- **代码落点**：Rust 后端 `src-tauri/src/`（commands/ + core/ 分层），Vue 前端 `src/`
  （api / components / composables / stores / views，视图不直连 invoke，走 `src/api/*.ts`）。
