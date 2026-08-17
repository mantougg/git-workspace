# GitWorkspace

> 高性能、多仓库 Git 工作空间 —— 为 AI Coding 时代而生。

[English](README.md) | [简体中文](README.zh-CN.md)

![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db)
![Vue 3](https://img.shields.io/badge/Vue-3-4FC08D)
![Rust](https://img.shields.io/badge/Rust-2021-dea584)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)

**GitWorkspace** 是基于 **Tauri 2 + Vue 3 + Rust** 构建的跨平台桌面应用，在一个界面中统一管理、扫描、操作**数十乃至数百个 Git 仓库**——从工作区级变更树与批量操作，到完整的 Git 客户端（分支、Stash、Merge/Rebase、冲突解决、Reflog、Worktree），再到工作区级自动化（Pipeline、任务 DAG、Manifest、统一 Undo），并内置 AI 辅助代码审查与完全离线的本地代码搜索。

## 为什么需要 GitWorkspace？

在 AI Coding 盛行的当下，代码的"生成"不再是瓶颈，瓶颈转移到了**跨大量仓库的变更管理**。单仓库 GUI 与 AI 编码工具一次只能看到一个根目录，看不到整个工作区。GitWorkspace 填补的正是这一层：

- **工作区优先**：项目根目录本身可以不是 Git 仓库，其中嵌套的几十个仓库会被自动发现、分组并统一管理；
- **确定性 + AI**：精确的状态 / Diff / 暂存是事实来源，AI 只在真正有价值处辅助（当前为代码审查，Roadmap 上还有提交信息、冲突解决、PR 描述）；
- **批量一切**：一次勾选文件或仓库，统一执行操作——带进度事件、取消能力、DAG 编排，以及兜底的统一 Undo 日志；
- **离线与私密**：本地 SQLite FTS5 代码搜索与审查流程，代码永远不需要离开你的机器。

## 功能特性

### 🗂️ 工作区与多仓库

- **工作区管理**：以目录为单位添加工作区，支持自定义扫描深度
- **并行仓库发现**：基于 rayon 的递归扫描，自动跳过 `node_modules`、`target`、`dist`、`build`、`.next`、`.nuxt`、`venv` 等目录
- **层级仓库分组**（`repo_groups`）：按分组浏览与筛选仓库
- **工作区 Dashboard 与 Health**（T-18 / T-19）：整个工作区的总览与健康检查
- **工作区 Manifest**（T-33）：基于清单的一键引导与批量 Clone
- **Change Set**（T-22）：可命名、可勾选的工作区级变更集

### 🧩 完整 Git 客户端

- **变更文件树（首页）**：`仓库 → 目录 → 文件` 三层树展示全部变更，节点可勾选，双击目录展开/折叠、双击文件查看 Diff，勾选状态实时汇总
- **批量 Git 操作**：`Add`（目录递归暂存，已删除文件自动移出索引）、`回退`（已跟踪文件从 HEAD 恢复、未跟踪文件删除）、`Pull` / `Fetch` / `Push`（走系统 `git` CLI，兼容 Windows Git Credential Manager 与 SSH 配置）、按仓库 `Commit`（支持文件选择、`amend`、提交并推送）
- **分支管理**（T-09）：新建 / 重命名 / 删除 / 切换 / 比较
- **Stash**（T-10）：push / pop / apply / drop，并支持**工作区级 Stash**（T-21）
- **Merge / Rebase**（T-15）：提供专用对话框
- **冲突解决器**（T-16）：ours / theirs / 手动编辑，冲突感知视图
- **Cherry-pick / Revert / Reset**（T-13）
- **Reflog**（T-14）：查看历史并找回丢失的提交
- **Worktree**（T-17）
- **Diff 查看器**：统一（Unified）与分栏（Side-by-Side）两种视图；支持 Hunk/行级暂存（T-12）与忽略选项（空白、大小写）
- **提交图**：SVG 泳道图展示提交历史，分支分出/汇入一目了然，合并提交以紫色圆点标识，支持分支/标签标记与分页加载

### ⚙️ 批量与自动化

- **后台任务队列**：8 个异步 worker，实时推送 `task_progress` 进度事件，支持取消、持久化历史与崩溃恢复
- **任务 DAG**（T-24）：依赖感知的编排，支持并行执行与部分失败语义
- **工作区 Pipeline**（T-23）：跨多仓库的编排工作流
- **统一 Undo / 操作日志**（T-34）：每个批量操作都有日志、可恢复
- **文件变更监听**（T-06）：基于 `notify` 轮询 + 500ms 去抖 → 增量刷新状态并推送 `repo_status_changed` 事件
- **Git 控制台**：fetch / pull / push 的 IDE 风格实时命令与输出（`git_command_result` 事件）

### 🤖 AI 与代码智能

- **AI 代码审查**：将工作区 diff 发送至任意 OpenAI 兼容 API（OpenAI、DeepSeek 等），返回 JSON 格式的 summary 与 issues（严重级别 / 类别 / 文件 / 描述）；diff 超 10k 字符自动截断；API Key 按请求传入、**永不落盘**
- **本地代码搜索**：SQLite FTS5 全文索引，跨仓库按相关度排序，支持按仓库重建/清除索引——完全离线，不依赖任何外部 AI 服务
- **敏感信息保护**（T-08）：日志与界面中的私钥、凭据自动脱敏
- **Roadmap 中**：AI 提交信息（T-25）、AI 冲突解决（T-26）、AI PR 描述 + 安全审查（T-27）

### 🚀 性能与可靠性

- **Rust 核心**：git2（libgit2）处理本地操作，系统 `git` 处理网络操作，tokio + rayon + dashmap + moka 缓存
- **SQLite WAL** 与单写者约束；任务历史跨重启持久化
- **Benchmark 门禁 CI**（`.github/workflows/benchmark.yml`）：每次推送强制校验性能阈值：
  - 100 个仓库的首次扫描 **< 2s**
  - 单仓库状态刷新 **< 100ms**
  - Diff 缓存命中 **< 50ms**
  - 提交图首屏 **< 1s**

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | [Tauri 2](https://tauri.app/)（插件：shell、dialog） |
| 前端 | Vue 3 + TypeScript + Vite 6 |
| UI 组件 | Element Plus + `@element-plus/icons-vue` |
| 状态管理 | Pinia |
| 路由 | Vue Router 4 |
| 后端 | Rust（edition 2021） |
| Git 本地操作 | [git2](https://crates.io/crates/git2) / libgit2（status、diff、commit、add、restore、分支、stash、merge、rebase、worktree） |
| Git 网络操作 | 系统 `git` CLI（fetch / pull / push，凭据与 SSH 取自系统配置） |
| 数据库 | SQLite（`rusqlite` 0.32 bundled；WAL + FTS5 全文索引） |
| 并发 | tokio + rayon + dashmap + moka |
| 文件监听 | `notify`（PollWatcher）+ 自研 500ms 去抖 |
| HTTP | reqwest（rustls-tls，AI 审查请求） |

## 项目结构

```
git-workspace/
├── index.html                  # 前端入口 HTML
├── package.json                # 前端依赖与脚本（pnpm）
├── vite.config.ts              # Vite 配置（端口 1420）
├── src/                        # 前端源码（Vue 3 + TS）
│   ├── main.ts / App.vue       # 入口 / 根组件（含任务面板）
│   ├── api/                    # Tauri command 封装（ai、batch、branch、changes、
│   │                           #   changeSet、commit、conflict、diff、git、git_ops、
│   │                           #   graph、group、health、history、logs、manifest、
│   │                           #   merge、operationLog、pipeline、rebase、reflog、
│   │                           #   repository、stash、task、workspace、workspaceStash、
│   │                           #   worktree）
│   ├── components/             # common / diff / graph / repo / branch 组件
│   ├── composables/            # useRepositories / useTaskProgress
│   ├── router/                 # /（变更树）、/diff、/graph、/branches、/stash 等
│   ├── stores/                 # Pinia stores（repository / task / workspace / changeSet）
│   ├── types/                  # TypeScript 类型定义
│   ├── utils/                  # format / error / frameTime 工具
│   └── views/                  # RepositoryList、DiffViewer、GitGraph、BranchManager、
│                               #   ConflictResolver、StashManager、WorktreeManager、
│                               #   Reflog、Dashboard、Health、ChangeSet、Pipeline、
│                               #   Manifest、OperationLog、TaskPanel
└── src-tauri/                  # Rust 后端（Tauri 2）
    ├── Cargo.toml              # Rust 依赖
    ├── tauri.conf.json         # Tauri 配置（窗口 / 打包）
    ├── capabilities/           # 插件权限声明
    └── src/
        ├── main.rs / lib.rs    # 入口；注册全部 Tauri commands
        ├── commands/           # command 层（workspace、repository、git_ops、diff、
        │                       #   graph、branch、stash、merge_rebase、conflict、reflog、
        │                       #   worktree、change_set、workspace_stash、health、history、
        │                       #   manifest、pipeline、operation_log、batch、ai、task 等）
        ├── core/               # 核心逻辑：scanner、git_ops、git_status、diff、graph、
        │                       #   branch、stash、merge、rebase、conflict、reflog、worktree、
        │                       #   change_set、workspace_stash、health、history、manifest、
        │                       #   pipeline、operation_log、selector、stage、secret、ssh、
        │                       #   watcher、logger
        ├── db/                 # SQLite（schema.rs / dao.rs）
        ├── models/             # 数据模型（workspace / repository / group / task 等）
        ├── task/               # 任务引擎（manager / queue / worker / dag）
        ├── benchmark/          # 性能基准（`cargo run --release --example benchmark`）
        ├── state.rs            # 应用全局状态
        └── error.rs            # 统一错误类型
```

## 快速开始

### 环境要求

- [Node.js](https://nodejs.org/) ≥ 18（推荐 20+）
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) stable 工具链（含 `cargo`）
- [Git](https://git-scm.com/) CLI —— 网络操作（fetch / pull / push）走系统 git，以使用系统凭据管理器与 SSH 配置
- Tauri 2 系统依赖：Windows 需 WebView2（Win10/11 一般自带）、macOS 需 Xcode CLT、Linux 需 `webkit2gtk-4.1` 等

### 开发模式

```bash
# 1. 安装前端依赖
pnpm install

# 2. 开发模式（启动 Vite 并编译 Rust，打开应用窗口）
pnpm tauri dev

# 仅启动前端（浏览器调试 UI，端口 1420）
pnpm dev
```

> `pnpm tauri dev` 首次运行会编译全部 Rust 依赖，耗时较长，之后为增量编译。

### 构建

```bash
# 类型检查 + 前端构建（输出到 dist/）
pnpm build

# 打包桌面应用（Windows 为 NSIS 安装包）
pnpm tauri build
```

### 性能基准

```bash
# 运行性能基准（例如 100 个仓库）
cargo run --release --example benchmark -- 100

# Diff / 提交图验收基准（T-04）
cargo run --release --example benchmark -- diff-graph
```

## Roadmap

开发按任务制路线推进，见 [`docs/`](docs/)。总体进度：**26 / 35 个任务（74%）**——Phase 0（基础稳定化）、Phase 1（完整 Git 客户端）、Phase 2（多仓库引擎）均已完成。

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| Phase 0 | 基础稳定化：Scanner、Status Engine、SQLite/WAL、任务队列、文件监听、基准、错误/日志/密钥保护 | ✅ 8/8 |
| Phase 1 | 完整 Git 客户端：分支、Stash、Commit/Diff 增强、Cherry-pick/Revert/Reset、Reflog、Merge/Rebase、冲突解决、Worktree | ✅ 9/9 |
| Phase 2 | 多仓库引擎：Dashboard、Health、批量操作、工作区 Stash/分支、Change Set、Pipeline、任务 DAG、Manifest、统一 Undo | ✅ 9/9 |
| Phase 3 | AI Git 助手：AI 提交信息、AI 冲突解决、AI PR 描述 + 安全审查 | ⬜ 0/3 |
| Phase 4/5/6 | 代码智能（符号索引）、远端平台集成、Submodule/LFS/Hooks、命令面板、插件系统、发布工程 | ⬜ 0/6 |

## 文档

- [任务拆解总览（`docs/tasks/README.md`）](docs/tasks/README.md)——35 个任务规格与验收标准
- [产品需求与技术架构 Roadmap](docs/GitWorkspace%20产品需求与技术架构%20Roadmap.md)
- [大型企业项目轻量级开发运行工作台](docs/大型企业项目轻量级开发运行工作台.md)

## 数据存储

- SQLite 数据库位于系统应用数据目录下的 `gitworkspace.db`
  - Windows：`%APPDATA%\com.gitworkspace.app`
  - macOS：`~/Library/Application Support/com.gitworkspace.app`
  - Linux：`~/.config/com.gitworkspace.app`
- 主要表：`workspaces`、`repositories`（收藏、标签、分组）、`repo_groups`、`task_history`、`code_index`（FTS5），以及操作日志、Change Set、Pipeline 与 Manifest 相关表

## 安全

- AI API Key 由前端按请求传入，**永不落盘**
- 私钥与凭据在日志和界面中自动脱敏（secret protection）
- AI 调用完全可选——应用可离线运行，不依赖任何外部服务

## License

[MIT](LICENSE)

## 署名

作者：**mantougg**
