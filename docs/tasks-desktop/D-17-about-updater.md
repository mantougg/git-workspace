# D-17 关于页 + 应用内更新

> **来源**：用户需求（2026-08-29）——设置分组新增「关于」页（应用图标 / 名称 / 版本号 / GitHub 地址 / 开源协议 / 检查更新按钮），打通 Tauri Updater 应用内更新，三平台无需手动下载安装包覆盖。
> **设计 spec**：UI 布局（§4）与技术方案（§5）以本文档为准。

| 项 | 值 |
|---|---|
| 阶段 | 增量 · 用户需求 |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | — |
| 对应方案 | 本文档 §4–§6（由评审稿 `../about-updater-plan.md` 迁入） |

---

## 1. 目标

SideNav「设置」分组新增「关于」页，并把已就绪的 Tauri Updater 链路接到前端：用户点击「检查更新」即可完成应用内更新并重启生效（Linux deb/rpm 除外，见 §6），无需再手动下载 exe / deb / dmg 覆盖安装。

## 2. 需求范围

- [x] 「关于」页（路由 `/about`，设置分组）：应用图标、名称、版本号、GitHub 仓库地址、开源协议（MIT）
- [x] 应用内更新状态机：检查 → 新版本提示（版本号 + 更新日志）→ 下载（进度条）→ 安装 → 重启生效
- [x] 失败兜底：error 提示 +「前往 GitHub Releases 手动下载」链接（覆盖 Linux deb/rpm 等无法应用内更新的场景）
- [x] 数据源遵循 F-07 延伸约定：协议 / 仓库地址经 package.json 字段构建期注入，组件内零硬编码
- [ ] 边界（本期不做）：启动时自动检查更新、国内镜像 endpoint、平台代码签名（Windows Authenticode / macOS 公证）

## 3. 现状：更新链路已就绪约 90%，只差前端入口

Tauri v2 Updater 的后端链路在本仓库已经全部配好，**不需要任何新增依赖**：

| 环节 | 状态 | 位置 |
|---|---|---|
| 更新产物生成 | ✅ | `tauri.conf.json` → `bundle.createUpdaterArtifacts: true` |
| 签名公钥 / endpoint | ✅ | `tauri.conf.json` → `plugins.updater`（endpoint 指向 `releases/latest/download/latest.json`） |
| Rust 插件注册 | ✅ | `src-tauri/src/lib.rs:118` → `tauri_plugin_updater` |
| 前端权限 | ✅ | `src-tauri/capabilities/desktop.json` → `updater:default` |
| 前端 JS 包 | ✅ | `package.json` → `@tauri-apps/plugin-updater`（已安装，未被使用） |
| CI 生成 latest.json | ✅ | `.github/workflows/release.yml`（`TAURI_SIGNING_PRIVATE_KEY` secrets + tauri-action 自动汇总上传） |
| **前端调用** | ❌ | 全项目搜不到 `check()` / `downloadAndInstall()` 调用——这就是本任务要补的 10% |

也就是说：当前发版时 latest.json 和各平台更新包已经会自动出现在 GitHub Release 里，只是应用内没有任何代码去消费它。

## 4. UI 设计（关于页）

遵循 desktop-skin 约定：`Panel` / `PanelHeader` 骨架 + tokens 变量，Naive UI 组件按需自动引入。

```
┌ 关于 ────────────────────────────────────────────────┐
│                                                      │
│   [图标 64px]   GitWorkspace                          │
│                 v0.2.0 by mantougg                    │
│                 高性能多仓库 Git 可视化管理平台（可选） │
│                                                      │
│   ── 应用信息 ──────────────────────────────────────  │
│   GitHub 仓库   https://github.com/mantougg/…    ↗   │
│   开源协议      MIT License                       ↗   │
│                                                      │
│   ── 软件更新 ──────────────────────────────────────  │
│   当前版本 v0.2.0                     [ 检查更新 ]     │
│                                                      │
│   状态区（按状态机切换，见 §5.2）：                     │
│   · 已是最新    → info alert「当前已是最新版本」        │
│   · 发现新版本  → 新版本号 + 更新日志 + [下载并安装]     │
│   · 下载中      → n-progress 百分比进度                │
│   · 下载完成    → 「重启应用以完成安装」+ [立即重启]     │
│   · 失败        → error alert + [前往 GitHub Releases] │
│                                                      │
└──────────────────────────────────────────────────────┘
```

「GitHub 仓库 / 开源协议 / 前往发布页」均通过 `open()`（`@tauri-apps/plugin-shell`，已有 `shell:allow-open` 权限，`WorktreeManager.vue` 已有同款用法）在系统浏览器打开。

## 5. 技术方案

### 5.1 路由与导航

- `src/router/index.ts`：「设置」分组下新增 `{ path: "/about", name: "about", component: AboutView, meta: { group: "设置", title: "关于" } }`。
- `src/components/shell/SideNav.vue`：`ICON_MAP` 增加 `about: InformationCircleOutline`（`@vicons/ionicons5` 自带）。SideNav 按 router meta 自动生成分组，无需其他改动。

### 5.2 useUpdater composable（`src/composables/useUpdater.ts`）

封装 `@tauri-apps/plugin-updater`，视图只消费状态：

```
idle ──check()──► checking ──┬─► null ────────► upToDate（提示已是最新）
                             └─► Update ──────► available(version/body)
available ──downloadAndInstall()──► downloading(进度) ──► ready
任意阶段出错 ──────────────────────────────────────────► error(兜底链接)
ready ──invoke("restart_app")──► 应用重启，新版本生效
```

- 进度：`DownloadEvent` 为 `{event:"Started", data:{contentLength?}} | {event:"Progress", data:{chunkLength}} | {event:"Finished"}`，`Started` 记总长、`Progress` 累加已收字节，算百分比；`contentLength` 缺失时进度条用 indeterminate。
- `Update` 是 `Resource`，组件卸载且未安装时调用 `close()` 释放。
- 错误信息原样透出（含网络失败），不吞错。

### 5.3 重启：自定义 `restart_app` 命令（零新增依赖）

`@tauri-apps/api` **没有** `restart()`（已核实 `node_modules/@tauri-apps/api/app.d.ts`），重启需要 Rust 侧配合。两个选项：

- **选定：自定义命令**（3 行代码，沿用本项目 commands 模块惯例，不加任何依赖）：
  - 新增 `src-tauri/src/commands/app.rs`：`#[tauri::command] pub fn restart_app(app: tauri::AppHandle) { app.restart(); }`
  - `tauri::AppHandle::restart(&self) -> !` 已确认存在于本地 tauri 2.11.5（`app.rs:588`）。
  - `commands/mod.rs` 加 `pub mod app;`，`lib.rs` 的 `generate_handler!` 注册 `commands::app::restart_app`。
  - 自定义命令不走 capabilities ACL（仅插件/核心命令需要），无需改权限文件。
- 备选（不采用）：`tauri-plugin-process` 官方插件——需同时加 Cargo 依赖 + npm 包 + capability，为一个 relaunch 不划算。

### 5.4 数据源约定（F-07 的自然延伸）

沿用「单一数据源、构建期注入、禁止组件硬编码」原则（AGENTS.md F-07）：

| 展示项 | 数据源 | 注入方式 |
|---|---|---|
| 版本号 `v0.2.0` | `package.json#version` | 复用现有 `__APP_VERSION__`，**不改** |
| 作者 `by mantougg` | `package.json#author` | 复用现有 `__APP_AUTHOR__`，**不改** |
| 开源协议 `MIT License` | `package.json` 新增 `"license": "MIT"` 字段 | `vite.config.ts` define 新增 `__APP_LICENSE__`，`vite-env.d.ts` 补声明 |
| 仓库地址 | `package.json` 新增 `"repository": "github:mantougg/git-workspace"` 字段 | define 新增 `__APP_REPOSITORY__`；发布页链接派生为 `` `${__APP_REPOSITORY__}/releases/latest` `` |
| 应用名称 `GitWorkspace` | 静态文案（与 SideNav 品牌区同源约定，产品名非元数据） | 组件内常量 |
| 应用图标 | `src-tauri/icons/128x128.png` 复制为 `src/assets/app-icon.png` | `import` 引入；图标变更时需重新拷贝（在视图注释中标注来源） |

### 5.5 更新失败 / Linux 边界的兜底

- **任何平台**检查或安装失败：error alert 附「前往 GitHub Releases 手动下载」链接。
- **Linux 已知硬限制**：Tauri Updater 仅支持 **AppImage** 安装方式的应用内更新；**deb/rpm 安装的用户无法应用内更新**（Tauri 官方限制，无法绕过）。deb/rpm 用户统一走兜底链接手动下载新包。本期不做安装方式探测，错误兜底天然覆盖（AppImage 用户成功、deb/rpm 用户安装失败后看到手动链接）。

## 6. 平台差异与已知限制

| 平台 | 应用内更新 | 说明 |
|---|---|---|
| Windows (NSIS) | ✅ | 下载 NSIS 包静默安装覆盖，体验最完整；SmartScreen 警告依旧（未做 Authenticode） |
| macOS | ✅ | 覆盖 .app；Gatekeeper 现状不变（未公证） |
| Linux (AppImage) | ✅ | 原地替换 AppImage |
| Linux (deb/rpm) | ❌ | Tauri 硬限制，走手动下载兜底 |

- **dev 模式行为**：`pnpm dev` 下可检查（真实请求 latest.json），但安装不适用于未打包环境——失败属预期，验收以打包版为准。
- **端到端验证窗口**：更新链路需要「旧版本已安装 + 新版本已发布」两个真实版本才能完整走通；本期交付验证到「检查到新版本 + 下载进度 + 提示重启」，最终端到端在下一个正式版本发布后回归确认。

## 7. 涉及文件清单

**新增（4 文件）：**

| 文件 | 说明 |
|---|---|
| `src/views/AboutView.vue` | 关于页视图（Panel 骨架 + 信息区 + 更新区） |
| `src/composables/useUpdater.ts` | 更新状态机 composable |
| `src/assets/app-icon.png` | 从 `src-tauri/icons/128x128.png` 复制 |
| `src-tauri/src/commands/app.rs` | `restart_app` 命令 |

**修改（8 文件，均为声明式接线，不动既有函数逻辑）：**

| 文件 | 改动 |
|---|---|
| `src/router/index.ts` | +`/about` 路由 |
| `src/components/shell/SideNav.vue` | +图标 import、+`ICON_MAP.about` |
| `src-tauri/src/commands/mod.rs` | +`pub mod app;` |
| `src-tauri/src/lib.rs` | `generate_handler!` +`restart_app` |
| `vite.config.ts` | define +`__APP_LICENSE__` / `__APP_REPOSITORY__` |
| `src/vite-env.d.ts` | +两个常量声明 |
| `package.json` | +`license` / `repository` 字段 |
| 本 README | D-17 行 + 进度计数（已随任务录入完成） |

**零新增 npm / Cargo 依赖。**

## 8. 验收标准

- [x] SideNav「设置」分组出现「关于」入口，图标/高亮行为与其他条目一致
- [x] 关于页正确展示：图标、名称、`v<package.json version> by <author>`、GitHub 链接（浏览器打开）、MIT License
- [x] 「检查更新」各状态正确：检查中禁用 → 无更新提示 → 有更新显示版本号/日志/下载按钮 → 进度条 → 提示重启 → `restart_app` 重启
- [x] 检查/下载失败时出现手动下载兜底链接
- [x] `pnpm build`（含 vue-tsc）与 `cargo check` 通过
- [x] 版本号/作者/协议/仓库地址无组件内硬编码（符合 F-07 及 §5.4 数据源表）

## 9. 决策点（已选方案记录）

| # | 决策 | 选定 | 备选 |
|---|---|---|---|
| 1 | 重启实现 | 自定义 `restart_app` 命令（零依赖） | `tauri-plugin-process` 官方插件 |
| 2 | 协议/仓库地址数据源 | `package.json` 字段 + 构建期注入（延续 F-07） | 组件内硬编码常量 |
| 3 | 关于页一句话简介 | 不加（保持需求最小集） | 加「高性能多仓库 Git 可视化管理平台」（静态文案） |
| 4 | StatusBar 版本号点击跳转关于页 | 不做（可选增量，一行改动） | 做 |

## 10. 风险与回滚

- 风险低：不动任何既有业务代码与 IPC；updater 插件本就注册，前端不调 `check()` 时行为与现状完全一致。
- 回滚：删除 `/about` 路由与新增文件即可，无数据迁移、无构建配置破坏。
- GitNexus：本会话未接入 GitNexus MCP（CLI 仅含索引管理命令）；改动均为新增文件 + 声明式注册，不修改既有函数逻辑。提交前如需，可跑 `node .gitnexus/run.cjs analyze` 刷新索引后复核。

---

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-29 完成实现与验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-29 | ⬜ | 方案评审通过，由 `../about-updater-plan.md` 迁入并转任务文档格式，登记为 D-17 |
| 2026-08-29 | ✅ | 完成：新增 About 页与 Tauri Updater 前端状态机，接入 `/about` 导航、构建期元数据、外链兜底及 `restart_app` Rust 命令；复制应用图标。验证：`pnpm build`、`cargo check` 通过；GitNexus `detect_changes` 因沙箱内 `spawnSync git` EPERM 未能运行。 |
