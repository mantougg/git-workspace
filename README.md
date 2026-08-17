# GitWorkspace

高性能多仓库 Git 可视化管理平台。基于 **Tauri 2 + Vue 3 + Rust** 构建的跨平台桌面应用，用于在一个界面中统一管理、扫描、操作多个 Git 仓库。

## 功能特性

- **工作区管理**：以目录为单位添加多个工作区，集中管理其中包含的所有 Git 仓库，支持自定义扫描深度
- **多线程仓库扫描**：递归扫描工作区目录，自动发现 `.git` 仓库（自动跳过 `node_modules`、`target`、`dist`、`build`、`.next`、`.nuxt`、`venv` 等目录），使用 rayon 并行校验
- **仓库分组**：支持层级分组（`repo_groups`），按分组浏览和筛选仓库
- **变更文件树（首页）**：以 仓库 → 目录 → 文件 三层树展示全部变更，节点可勾选，双击目录展开/折叠、双击文件在右侧查看该文件 diff；勾选状态实时汇总
- **批量 Git 操作**：底部常驻操作面板支持对勾选文件/仓库批量执行
  - `Add`（暂存，目录递归，已删除文件自动从索引移除）
  - `回退`（还原工作区变更：已跟踪文件从 HEAD 恢复，未跟踪/新增文件从磁盘删除）
  - `Pull` / `Fetch` / `Push`（网络操作走系统 `git` CLI，兼容 Windows Git Credential Manager 与 SSH 配置）
  - `Commit`（按仓库分别提交，可指定文件列表，留空则提交全部变更）
- **后台任务队列**：内置 8 个 worker 的异步任务池，批量操作后台执行，实时推送 `task_progress` 进度事件，支持取消、历史记录（`task_history`），任务完成后自动清理
- **Git 控制台**：网络操作（fetch/pull/push）实时推送 `git_command_result` 事件，前端以 IDE 风格展示执行的命令与输出
- **文件变更监听**：基于 `notify`（轮询模式）实时监听仓库文件变化，500ms 去抖后增量刷新状态并推送 `repo_status_changed` 事件
- **Diff 查看器**：支持统一（Unified）与分栏（Side-by-Side）两种视图查看工作区改动
- **提交图（Git Graph）**：SVG 泳道图展示提交历史，分支分出/汇入（合并）一目了然，合并提交以紫色圆点标识，支持分支/标签标记与分页加载
- **AI 代码审查**：将工作区 diff 发送至 OpenAI 兼容 API（如 OpenAI、DeepSeek 等）自动审查，返回问题清单（严重级别/类别/文件/描述），diff 超 10k 字符自动截断
- **代码搜索**：内置 SQLite FTS5 全文索引（`code_index`），跨仓库本地全文搜索代码内容并按相关度排序，支持按仓库重建/清除索引（不依赖外部 AI 服务）

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | [Tauri 2](https://tauri.app/)（插件：shell、dialog） |
| 前端 | Vue 3 + TypeScript + Vite 6 |
| UI 组件 | Element Plus + `@element-plus/icons-vue` |
| 状态管理 | Pinia |
| 路由 | Vue Router 4 |
| 后端语言 | Rust（edition 2021） |
| Git 本地操作 | [git2](https://crates.io/crates/git2)（libgit2：commit/add/restore/status/diff） |
| Git 网络操作 | 系统 `git` CLI（fetch/pull/push，兼容凭据管理器与 SSH） |
| 数据库 | SQLite（`rusqlite` 0.32，bundled；含 FTS5 全文索引） |
| 并发 | tokio + rayon + dashmap |
| 文件监听 | notify（PollWatcher 轮询）+ 自研 500ms 去抖 |
| HTTP | reqwest（rustls-tls，AI 审查请求） |

## 项目结构

```
git-multi/
├── index.html                  # 前端入口 HTML
├── package.json                # 前端依赖与脚本（pnpm）
├── vite.config.ts              # Vite 配置（端口 1420）
├── tsconfig.json               # TypeScript 配置
├── src/                        # 前端源码（Vue 3）
│   ├── main.ts                 # 应用入口
│   ├── App.vue                 # 根组件（含任务面板）
│   ├── api/                    # Tauri command 调用封装
│   │   ├── ai.ts               #   AI 审查 / 代码索引构建 / 搜索 / 清除索引
│   │   ├── changes.ts          #   工作区变更文件树 / 批量 add / 批量回退
│   │   ├── git.ts              #   Diff 查询
│   │   ├── git_ops.ts          #   批量 fetch/pull/push/commit、同步操作、监听器
│   │   ├── graph.ts            #   提交历史 / 分支
│   │   ├── group.ts            #   仓库分组管理
│   │   ├── repository.ts       #   扫描 / 仓库列表 / 状态刷新
│   │   ├── workspace.ts        #   工作区管理
│   │   └── task.ts             #   任务提交 / 状态 / 取消
│   ├── components/             # 通用组件
│   │   ├── common/             #   GroupTree / SearchBar / WorkspaceManager
│   │   ├── diff/               #   UnifiedDiff / SideBySideDiff
│   │   ├── graph/              #   CommitGraph（SVG 泳道图）
│   │   └── repo/               #   ChangeTree / BatchActionBar / RepoCard / RepoTable / StatusBadge
│   ├── composables/            # 组合式函数（useRepositories / useTaskProgress）
│   ├── router/                 # 路由（/ 变更树首页、/diff、/graph）
│   ├── stores/                 # Pinia stores（repository / task / workspace）
│   ├── types/                  # TypeScript 类型定义
│   ├── utils/format.ts         # 格式化工具
│   └── views/                  # 页面视图
│       ├── RepositoryList.vue  #   变更文件树首页（勾选 + 批量操作面板）
│       ├── DiffViewer.vue      #   Diff 查看器
│       ├── GitGraph.vue        #   提交图
│       └── TaskPanel.vue       #   任务面板
└── src-tauri/                  # Rust 后端（Tauri）
    ├── Cargo.toml              # Rust 依赖
    ├── tauri.conf.json         # Tauri 配置（窗口 / 打包）
    ├── capabilities/           # 插件权限声明
    └── src/
        ├── main.rs / lib.rs    # 入口，注册全部 Tauri commands
        ├── commands/           # Tauri command 层（workspace/repository/git_ops/diff/graph/task/ai）
        ├── core/               # 核心业务逻辑
        │   ├── scanner.rs      #   多线程仓库扫描器
        │   ├── git_ops.rs      #   Git 操作（CLI 网络操作 + libgit2 本地操作，含 batch_add/batch_restore）
        │   ├── git_status.rs   #   仓库状态检测 + 文件级变更（get_repo_changes）
        │   ├── diff.rs         #   Diff 解析
        │   ├── graph.rs        #   提交图数据
        │   ├── ssh.rs          #   SSH 凭据管理（预留 libgit2 认证路径）
        │   └── watcher.rs      #   文件变更监听
        ├── db/                 # SQLite（schema.rs / dao.rs）
        ├── models/             # 数据模型（workspace/repository/group/task）
        ├── task/               # 后台任务队列（manager/queue/worker）
        ├── state.rs            # 应用全局状态
        └── error.rs            # 统一错误类型
```

## 环境要求

- [Node.js](https://nodejs.org/) ≥ 18（推荐 20+）
- [pnpm](https://pnpm.io/)（包管理）
- [Rust](https://www.rust-lang.org/) stable 工具链（含 `cargo`）
- [Git](https://git-scm.com/) CLI（网络操作 fetch/pull/push 依赖系统 git，以使用系统凭据管理器与 SSH 配置）
- Tauri 2 系统依赖：Windows 需 [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)（Win10/11 一般自带）；macOS 需 Xcode CLT；Linux 需 `webkit2gtk-4.1` 等

## 快速开始

```bash
# 1. 安装前端依赖
pnpm install

# 2. 开发模式（启动 Vite 并编译 Rust，打开应用窗口）
pnpm tauri dev

# 仅启动前端（浏览器中调试 UI，端口 1420）
pnpm dev
```

> `pnpm tauri dev` 首次运行会编译全部 Rust 依赖，耗时较长，之后为增量编译。

## 构建

```bash
# 类型检查 + 前端构建（输出到 dist/）
pnpm build

# 打包桌面应用（Windows 为 NSIS 安装包）
pnpm tauri build
```

## 数据存储

- 数据库文件：SQLite，位于系统应用数据目录下的 `gitworkspace.db`
  - Windows：`%APPDATA%\com.gitworkspace.app`
  - macOS：`~/Library/Application Support/com.gitworkspace.app`
  - Linux：`~/.config/com.gitworkspace.app`
- 主要表：
  - `workspaces`（工作区，含扫描深度）
  - `repositories`（仓库，含收藏标记 `is_favorite`、标签 `tags`、分组 `group_id`）
  - `repo_groups`（层级仓库分组）
  - `task_history`（任务历史）
  - `code_index`（FTS5 代码搜索索引）

## AI 功能说明

- **AI 代码审查**（`ai_review`）：获取仓库工作区 diff（超 10k 字符自动截断），调用 OpenAI 兼容的 `/v1/chat/completions` 接口（默认模型 `gpt-4o-mini`，可通过 `api_url` 指定其他服务），返回 JSON 格式的 `summary` 与 `issues`（严重级别 / 类别 / 文件 / 描述）
- **代码索引与搜索**（`build_code_index` / `ai_search` / `clear_code_index`）：将仓库内文本文件（跳过二进制、大文件与构建产物目录）写入 SQLite FTS5 全文索引，`ai_search` 基于本地全文匹配按相关度返回结果（最多 50 条），不调用外部 AI 服务
- API Key 由前端传入，不落盘存储

## License

私有项目（`private: true`），未指定开源许可证。

## 署名

- **作者：mantougg**
