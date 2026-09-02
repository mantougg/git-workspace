# GitWorkspace

> 面向"一个项目拆成几十个 Git 仓库"的桌面开发工作台 —— 跨仓库批量 Git 操作、不启动 IDE 构建/运行 Spring Boot 与 Node.js 服务、自带 Key 的 AI 辅助审查。免费（MIT）、离线优先，基于 Tauri 2 + Vue 3 + Rust，100 个仓库扫描 2 秒内完成。

[English](README.md) | [简体中文](README.zh-CN.md)

![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24c8db)
![Vue 3](https://img.shields.io/badge/Vue-3-4FC08D)
![Rust](https://img.shields.io/badge/Rust-2021-dea584)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)

**GitWorkspace** 是为"在同一个项目里面对**数十乃至数百个独立 Git 仓库**"的工程师打造的跨平台桌面应用。它把通常需要四类工具才能覆盖的能力合在一起：

1. **多仓库工作区引擎** —— 自动发现、分组并监控一个根目录下所有嵌套仓库；整个工作区的变更汇成一棵树；批量操作经任务队列编排，附带统一 Undo 日志兜底。
2. **完整 Git 客户端** —— 分支、Stash、交互式 Rebase（拖拽排序）、Merge、冲突解决、Cherry-pick / Revert / Reset、Reflog、Worktree、提交图、Hunk/行级暂存。
3. **Runtime 工作台** —— 不打开 IntelliJ / VS Code 即可构建、运行、停止并监控 Spring Boot 与 Node.js 服务：JDK / Maven / Node 工具链管理、流式日志、健康探针、端口管理、增量构建与自动重启。
4. **AI 助手** —— 自带 Key 即用（OpenAI Chat/Responses、Anthropic Messages 或任意兼容网关）：AI 代码审查、AI 提交信息、AI 冲突解决、AI PR 描述与安全审查。所有写操作执行前都有预览确认。

## 为什么需要 GitWorkspace？

在 AI Coding 盛行的当下，代码的"生成"不再是瓶颈，瓶颈转移到了**跨大量仓库的变更管理**与**大量本地服务的运行**。单仓库 Git GUI 与 IDE 一次只能看到一个根目录，而 GitWorkspace 以整个工作区为单位工作：

- **工作区优先** —— 项目根目录本身可以不是 Git 仓库，其中嵌套的几十个仓库会被自动发现、分组并统一管理；
- **确定性内核，AI 在上层** —— 精确的状态 / Diff / 暂存是事实来源，AI 只在真正有价值处辅助，且写操作永远先预览；
- **批量一切** —— 一次勾选文件或仓库、统一执行，带进度事件、取消能力、DAG 编排与可回溯的统一操作日志；
- **离线与私密** —— 扫描、Diff、构建、运行、日志全部本地完成。AI 调用完全可选，使用你自己的 API Key，且密钥在请求发出前自动脱敏。

## 适合 / 不适合

**适合**

- 项目拆分在**大量独立 Git 仓库**中的团队（微服务、平台型代码库）
- 需要在本地**构建 / 运行 / 停止 Spring Boot 或 Node.js 服务**、但不打算打开完整 IDE 的开发、测试、联调与运维角色
- 每天都要在多个仓库之间做**批量 Git 操作**（fetch / pull / commit / push）的人

**不适合**

- 深度单仓库工作流——Fork、Sublime Merge、GitKraken 可能更合适
- 代码编辑——GitWorkspace 与 IDE 是互补关系，不是替代关系

## 功能特性

### 🗂️ 工作区与多仓库引擎

- **工作区管理**：以目录为单位添加工作区，支持自定义扫描深度
- **并行仓库发现**：基于 rayon 的递归扫描，自动跳过 `node_modules`、`target`、`dist`、`build`、`.next`、`venv` 等目录
- **层级仓库分组**：按分组浏览与筛选仓库
- **工作区 Dashboard 与健康检查**：整个工作区的总览与异常检测（detached HEAD、LFS/Submodule 异常、陈旧分支等）
- **工作区 Manifest**：把仓库清单导出为 JSON，在任何机器一键批量 Clone——团队新成员入项目一步到位
- **Change Set**：可命名、可勾选的工作区级变更集
- **提交热力图**：GitHub 风格的贡献热力图，聚合工作区内全部仓库

### 🧩 完整 Git 客户端

- **变更文件树（首页）**：`仓库 → 目录 → 文件` 三层树展示全部变更，节点可勾选，双击查看 Diff，勾选状态实时汇总
- **批量 Git 操作**：`Add` / `回退` / `Pull` / `Fetch` / `Push` / `Commit` 一次作用于多个仓库（网络操作走系统 `git` CLI，兼容 Windows Credential Manager 与 SSH 配置）
- **分支管理**：新建 / 重命名 / 删除 / 切换 / 比较
- **Stash**：push / pop / apply / drop，并支持**工作区级 Stash**
- **交互式 Rebase**：拖拽排序提交，支持 pick / reword / squash / drop，continue / skip / abort 与冲突解决器联动
- **Merge 与冲突解决器**：ours / theirs / 手动编辑，冲突感知视图
- **Cherry-pick / Revert / Reset** · **Reflog**（找回丢失提交）· **Worktree**
- **Diff 查看器**：统一与分栏两种视图，Hunk/行级暂存，空白/大小写忽略选项
- **提交图**：SVG 泳道图，分支/标签标记，分页加载
- **三栏变更视图**：仓库树、提交图与 Diff 三栏联动

### 🛠️ Runtime 工作台 —— 不启动 IDE 跑服务

- **Spring Boot**：Maven/POM 发现、Main Class 推断、运行时闭包分析、构建引擎（`mvn` / `mvnw`，支持 **mvnd 守护进程与构建缓存**加速）、启动管理（优雅停止、进程树终止）
- **Node.js**：工具链检测（含 nvm / fnm / volta / mise 等版本管理器）、包管理器决策链（npm / pnpm / yarn）、dev server 启动与端口预检
- **多服务环境**：定义、一键启停一组服务
- **日志引擎**：按服务流式输出、可检索
- **健康探针**：Port / HTTP / TCP / Spring Boot Actuator 四种，持续监控
- **端口管理**：查看端口占用进程、安全释放、修改配置端口（独立工具页）
- **文件监听 → 增量构建 → 自动重启**
- **Runtime 模板与启动预设**：按服务配置环境变量、JDK 覆盖、JVM/Node 参数（含 IDEA 风格 Spring Boot 启动参数预设）
- **跨仓库依赖图可视化**，并与 Git 联动（状态提示、运行中服务的操作保护）

### 🤖 AI 助手（可选，自带 Key）

- **Provider / 模型 / 凭据管理**：支持 OpenAI Chat Completions、OpenAI Responses 与 Anthropic Messages 三种协议——OpenAI、Anthropic、DeepSeek 及任意兼容网关均可接入
- **AI 代码审查**：对工作区 diff 输出结构化问题列表（严重级别 / 类别 / 文件）
- **AI 提交信息 · AI 冲突解决 · AI PR 描述**：附安全审查 / 缺陷检测 / 提交解读
- **助手抽屉**：以只读 Git 与 Runtime 工具（状态、Diff、日志、本地 FTS5 代码检索）对话，并给出**行动提案**——所有写操作执行前先预览
- **隐私设计**：提示词发出前自动脱敏；API Key 存入**操作系统钥匙串**（Windows Credential Manager / macOS Keychain / Secret Service）

### 🖥️ 桌面体验

- **命令面板**（`Ctrl/Cmd+K`）：分组导航与操作
- **键盘快捷键**（`Ctrl+1..9` 切换视图等）
- **主题**：暗色 / 亮色双套 Design Tokens，自动跟随系统外观并持久化
- **窗口状态记忆、面板分割位置记忆、右键菜单**
- **关于页与应用内更新器** —— CI 构建三平台安装包（Windows / macOS / Linux）

### ⚙️ 批量与自动化

- **后台任务队列**：异步 worker，实时推送 `task_progress` 进度事件，支持取消、持久化历史与崩溃恢复
- **任务 DAG**：依赖感知编排，并行执行与部分失败语义
- **工作区 Pipeline**：跨多仓库的编排工作流
- **统一 Undo / 操作日志**：每个批量操作都有日志、可恢复
- **文件变更监听**：基于 `notify` + 去抖 → 增量刷新状态（合并推送 `repo_status_changed` 事件）
- **Git 控制台**：fetch / pull / push 的 IDE 风格实时命令与输出

### ⚡ 性能与可靠性

- **Rust 核心**：git2（libgit2）处理本地操作，系统 `git` 处理网络操作，tokio + rayon + dashmap + moka 缓存
- **SQLite WAL** 与单写者约束；任务历史跨重启持久化
- **Benchmark 门禁 CI**（`.github/workflows/benchmark.yml`）：每次推送强制校验性能阈值：
  - 100 个仓库的首次扫描 **< 2s**
  - 单仓库状态刷新 **< 100ms**
  - Diff 缓存命中 **< 50ms**
  - 提交图首屏 **< 1s**

## 与同类工具对比

| | GitWorkspace | GitKraken / Fork / Sourcetree | IntelliJ IDEA / VS Code |
| --- | --- | --- | --- |
| 工作单位 | 一个工作区内的多个仓库 | 一次一个仓库（GitKraken Workspaces 提供批量 fetch/pull） | 一个工程 |
| 跨仓库批量操作 | ✅ 核心能力 | 有限 | ❌ |
| 构建/运行服务、日志、端口、健康检查 | ✅ 内置 | ❌ | ✅ 但重量级 |
| 不打开 IDE 即可工作 | ✅ | ✅ | — |
| AI 审查 / 提交信息 / 冲突辅助 | ✅ 自带 Key | 部分（GitKraken AI） | 靠插件 |
| 授权与价格 | 免费，MIT | 免费档 / 付费 | 社区版 / 付费 |

在深度单仓库操作（精细 Blame、复杂历史手术）上，专业 Git GUI 依然优秀——GitWorkspace 在这一层是补充，不是替代。

## 常见问题

**怎么在几十个仓库上批量 pull / fetch / push？**
把项目根目录添加为工作区，勾选仓库（或全选），执行一次即可。所有操作走后台任务队列，有实时进度、可取消，并记录在可撤销的操作日志里。

**能不能不打开 IntelliJ / VS Code 就运行 Spring Boot / Node.js 服务？**
可以——这正是 Runtime 工作台。GitWorkspace 检测你的 JDK / Maven / Node 工具链，推断每个应用的构建与启动方式，流式输出日志、运行健康探针、管理端口。它面向的是测试、联调、运维这类不需要写代码的角色（以及 AI 辅助流程）。

**我的代码会离开本机吗？**
不会。扫描、状态、Diff、检索、构建、运行全部本地完成。AI 功能严格可选：使用你自己的 API Key（存于系统钥匙串），请求发出前自动脱敏。

**收费吗？**
MIT 协议，免费，无需注册账号。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | [Tauri 2](https://tauri.app/)（插件：shell、dialog、updater） |
| 前端 | Vue 3 + TypeScript + Vite 6 |
| UI 组件 | Naive UI |
| 状态管理 | Pinia |
| 路由 | Vue Router 4 |
| 后端 | Rust（edition 2021） |
| Git 本地操作 | [git2](https://crates.io/crates/git2) / libgit2 |
| Git 网络操作 | 系统 `git` CLI（凭据与 SSH 取自系统配置） |
| 数据库 | SQLite（`rusqlite` bundled；WAL） |
| AI | reqwest（rustls-tls）；OpenAI Chat/Responses + Anthropic Messages 协议；`keyring` 系统钥匙串存储 |
| 并发 | tokio + rayon + dashmap + moka |
| 文件监听 | `notify` + 去抖 |

## 下载

CI 会把预构建安装包（Windows NSIS 安装程序、macOS 与 Linux bundle）发布到 [Releases](https://github.com/mantougg/git-workspace/releases) 页面。Windows 上可能出现 SmartScreen"未知发布者"提示（二进制未做代码签名），含义与处理见 [docs/release.md](docs/release.md)。

## 从源码构建

### 环境要求

- [Node.js](https://nodejs.org/) ≥ 18（推荐 20+）与 [pnpm](https://pnpm.io/)
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

### 构建与打包

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

# Diff / 提交图验收基准
cargo run --release --example benchmark -- diff-graph
```

## Roadmap

开发按任务制路线推进，见 [`docs/`](docs/)。核心进度：**29 / 35 个任务（83%）**——Phase 0~3 全部完成；桌面壳层改造、Runtime 工作台、Node.js 运行时与 AI 助手轨道均已交付。

| 阶段 | 范围 | 状态 |
| --- | --- | --- |
| Phase 0 | 基础稳定化：Scanner、Status Engine、SQLite/WAL、任务队列、文件监听、基准、错误/日志/密钥保护 | ✅ 8/8 |
| Phase 1 | 完整 Git 客户端：分支、Stash、Commit/Diff 增强、Cherry-pick/Revert/Reset、Reflog、Merge/Rebase、冲突解决、Worktree | ✅ 9/9 |
| Phase 2 | 多仓库引擎：Dashboard、Health、批量操作、工作区 Stash/分支、Change Set、Pipeline、任务 DAG、Manifest、统一 Undo | ✅ 9/9 |
| Phase 3 | AI Git 助手：AI 提交信息、AI 冲突解决、AI PR 描述 + 安全审查 | ✅ 3/3 |
| Phase 4/5/6 | 代码智能（符号索引）、远端平台集成（PR/CI）、Submodule/LFS/Hooks、插件系统、发布工程 | ⬜ 0/6 |
| 并行轨道 | 桌面壳层（D-01~17）、Runtime 工作台（R-01~21）、Node.js 运行时（N-01~10） | ✅ 完成 |

Runtime 后续规划：Gradle 支持、Debug 协同（JDWP）、Docker/Kubernetes Runtime、JVM 监控指标。

## 文档

- [任务拆解总览（`docs/tasks/README.md`）](docs/tasks/README.md)——核心任务规格与验收标准
- [发布与代码签名（`docs/release.md`）](docs/release.md)
- [产品需求与技术架构 Roadmap](docs/GitWorkspace%20产品需求与技术架构%20Roadmap.md)
- [大型企业项目轻量级开发运行工作台](docs/大型企业项目轻量级开发运行工作台.md)
- 其他任务轨道：[`docs/tasks-desktop/`](docs/tasks-desktop/README.md) · [`docs/tasks-runtime/`](docs/tasks-runtime/README.md) · [`docs/tasks-node/`](docs/tasks-node/README.md) · [`docs/tasks-ai/`](docs/tasks-ai/)

## 数据存储

- SQLite 数据库位于系统应用数据目录下的 `gitworkspace.db`
  - Windows：`%APPDATA%\com.gitworkspace.app`
  - macOS：`~/Library/Application Support/com.gitworkspace.app`
  - Linux：`~/.config/com.gitworkspace.app`
- 主要表：`workspaces`、`repositories`（收藏、标签、分组）、`repo_groups`、`task_history`、Runtime 与前端工程索引表，以及操作日志、Change Set、Pipeline 与 Manifest 相关表

## 安全

- AI 完全可选、默认关闭——应用可离线运行，不依赖任何外部服务
- API Key 存储于**操作系统钥匙串**（`keyring`），永不写入明文文件
- 私钥与凭据在日志、界面和 AI 提示词中自动脱敏（secret protection）

## License

[MIT](LICENSE)

## 署名

作者：**mantougg**
