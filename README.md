# GitWorkspace

高性能多仓库 Git 可视化管理平台。基于 **Tauri 2 + Vue 3 + Rust** 构建的跨平台桌面应用，用于在一个界面中统一管理、扫描、操作多个 Git 仓库。

## 功能特性

- **工作区管理**：以目录为单位添加多个工作区，集中管理其中包含的所有 Git 仓库
- **多线程仓库扫描**：递归扫描工作区目录，自动发现 `.git` 仓库（自动跳过 `node_modules`、`target`、`.next` 等目录），支持自定义扫描深度
- **仓库分组**：支持层级分组（`repo_groups`），按分组浏览和筛选仓库
- **批量 Git 操作**：对多个仓库一键执行 `fetch` / `pull` / `push` / `commit`，基于 libgit2（`git2` crate）实现，无需安装 Git CLI
- **变更文件树（首页）**：以 仓库 → 目录 → 文件 三层树展示全部变更，节点可勾选（仅复选框触发勾选），双击目录展开/折叠、双击文件在右侧查看该文件 diff；勾选状态实时汇总
- **批量 Git 操作**：底部常驻操作面板支持对勾选文件/仓库批量执行 `add`（暂存，目录递归）、`pull`、`push`、`commit`（底部输入 message，按仓库分别提交），全程走后台任务队列并实时推送进度
- **后台任务队列**：内置 8 个 worker 的异步任务池，批量操作后台执行，实时推送进度事件，支持取消与历史记录
- **SSH 凭据自动管理**：自动尝试 SSH Agent → `~/.ssh` 下的密钥（`id_ed25519` / `id_rsa` / `id_ecdsa` / `id_dsa`）→ HTTPS 默认凭据
- **Diff 查看器**：支持统一（Unified）与分栏（Side-by-Side）两种视图查看工作区改动
- **提交图（Git Graph）**：SVG 泳道图展示提交历史，分支分出/汇入（合并）一目了然，合并提交以紫色圆点标识
- **文件变更监听**：基于 `notify` 实时监听仓库文件变化，自动刷新状态
- **AI 代码审查**：将工作区 diff 发送至 OpenAI 兼容 API（如 OpenAI、DeepSeek 等）自动审查，返回问题清单（严重级别/类别/文件/描述）
- **AI 代码搜索**：内置 SQLite FTS5 全文索引（`code_index`），跨仓库搜索代码内容

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | [Tauri 2](https://tauri.app/) |
| 前端 | Vue 3 + TypeScript + Vite 6 |
| UI 组件 | Element Plus + `@element-plus/icons-vue` |
| 状态管理 | Pinia |
| 路由 | Vue Router 4 |
| 后端语言 | Rust（edition 2021） |
| Git 操作 | [git2](https://crates.io/crates/git2)（libgit2，支持 ssh/https） |
| 数据库 | SQLite（`rusqlite`，bundled） |
| 并发 | tokio + rayon + dashmap |
| 文件监听 | notify + notify-debouncer-full |
| HTTP | reqwest（rustls-tls） |

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
│   │   ├── ai.ts               #   AI 审查 / 代码索引 / 代码搜索
│   │   ├── changes.ts          #   工作区变更文件树 / 批量 add
│   │   ├── git_ops.ts          #   批量 fetch/pull/push/commit/add、监听器
│   │   ├── graph.ts            #   提交历史 / 分支
│   │   ├── repository.ts       #   扫描 / 仓库 / 分组
│   │   ├── workspace.ts        #   工作区管理
│   │   ├── task.ts             #   任务提交 / 状态
│   │   └── diff.ts             #   Diff 查询
│   ├── components/             # 通用组件
│   │   ├── common/             #   GroupTree / SearchBar / WorkspaceManager
│   │   ├── diff/               #   UnifiedDiff / SideBySideDiff
│   │   ├── graph/              #   CommitGraph（SVG 泳道图）
│   │   └── repo/               #   ChangeTree / StatusBadge
│   ├── composables/            # 组合式函数（useRepositories / useTaskProgress）
│   ├── router/                 # 路由（/ 变更树首页、/diff、/graph）
│   ├── stores/                 # Pinia stores（repository / task / workspace）
│   ├── types/                  # TypeScript 类型定义（含 changes.ts）
│   ├── utils/format.ts         # 格式化工具
│   └── views/                  # 页面视图
│       ├── RepositoryList.vue  #   变更文件树首页（勾选 + 底部批量操作）
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
        │   ├── git_ops.rs      #   libgit2 批量 Git 操作（含 batch_add）
        │   ├── git_status.rs   #   仓库状态检测 + 文件级变更（get_repo_changes）
        │   ├── diff.rs         #   Diff 解析
        │   ├── graph.rs        #   提交图数据
        │   ├── ssh.rs          #   SSH 凭据管理
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

# 打包桌面应用（NSIS / MSI / dmg / AppImage 等，取决于平台）
pnpm tauri build
```

## 数据存储

- 数据库文件：SQLite，位于系统应用数据目录下的 `gitworkspace.db`
  - Windows：`%APPDATA%\com.gitworkspace.app`
  - macOS：`~/Library/Application Support/com.gitworkspace.app`
  - Linux：`~/.local/share/com.gitworkspace.app`
- 主要表：`workspaces`（工作区）、`repositories`（仓库）、`repo_groups`（仓库分组）、`task_history`（任务历史）、`code_index`（FTS5 代码搜索索引）

## AI 功能说明

- **AI 代码审查**（`ai_review`）：获取当前工作区 diff（超 10k 字符自动截断），调用 OpenAI 兼容的 `/v1/chat/completions` 接口（默认 `gpt-4o-mini`，可通过 `api_url` 指定其他服务），返回 JSON 格式的 `summary` 与 `issues`（严重级别 / 类别 / 文件 / 描述）
- **代码索引与搜索**（`build_code_index` / `ai_search`）：构建 FTS5 全文索引后，可跨仓库搜索代码片段并按相关性排序
- API Key 由前端传入，不落盘存储

## License

私有项目（`private: true`），未指定开源许可证。

## 署名

- **作者：wangyi**
