# F-10 桌面化 UI 改造方案（Desktop Skin）

> 来源：F-10 讨论结论（2026-08-27）。
> 目标：消除「Web 套壳感」，让 GitWorkspace 观感与交互对齐原生桌面客户端（参照 IDEA / Fork / SourceTree）。
> 范围说明：本方案只覆盖「桌面皮肤 + 布局骨架」，不自研组件库（见「非目标」）。
>
> 已定案的关键决策（2026-08-27 与用户确认）：
> 1. **Dashboard 保留为默认首页**（不再承担导航中转，纯数据总览）。
> 2. **SideNav 全量平铺**：所有导航条目图标+文字直出（含 Git 组 9 项），不做「更多」折叠。
> 3. **暗色主题一期就接入**：tokens 亮暗双套一次到位，避免二期大规模返工。
> 4. **任务型页面（Diff / 冲突解决器 / 新建应用向导）保持独立路由页面**，骨架提供统一返回。
>
> 二次修订（2026-08-27，依据方案评审，已与用户对齐）：
> 5. **SideNav 宽度 188px**（原 200px，Git 组 9 项下视觉重量过大）。
> 6. **Dashboard 去卡片墙**：8 张 stat-card 收敛为一条高密度摘要行（保留点击跳转语义）；不做信息流式 Overview / Recent Activity（后端无活动流数据源，违背最小改动）。
> 7. **StatusBar 收敛**：只保留 工作区｜分支｜watcher｜任务｜版本；主题三档切换移入设置页，不常驻状态栏。
> 8. **自定义无边框标题栏**：一期/二期明确不做；三期（D-16）已重估，**结论：暂不实施**（理由见 §2，非永久排除，4 期+ 可再评估）。
> 9. **新增 2.5 期「Desktop Interaction」**：Command Palette（Ctrl/Cmd+K）、ContextMenu、键盘快捷键从三期提前——对「桌面感」的杠杆高于三栏联动。
>
> **执行拆解（2026-08-27）**：本方案已拆分为 16 个可执行任务，见 [tasks-desktop/README.md](./tasks-desktop/README.md)（D-01~D-16）。**本文档是设计 spec（布局/视觉基准）；任务执行状态以 tasks-desktop 索引为准**，开发流程由 skill `gitworkspace-desktop-dev` 承载。

---

## 1. 现状诊断

| 问题 | 位置 | 说明 |
|---|---|---|
| 无持久导航骨架 | `src/App.vue` | 整个应用只有 `router-view + 底部版本栏`，22 个视图平铺，靠 Dashboard 工具栏按钮互相跳转——这是「网站」模型，不是「客户端」模型 |
| 样式体系未统一 | `src/views/DashboardView.vue:544` 等 | 大量使用 `var(--el-color-*)`（Element Plus 变量），但项目用的是 Naive UI，变量不存在；各视图硬编码色值/间距（`#ebeef5`、`#909399`…） |
| 组件默认皮肤偏「网页后台」 | 全局 | Naive UI 默认大圆角、大留白、默认字号，未经任何 themeOverrides 收敛 |
| Dashboard 卡片墙 | `src/views/DashboardView.vue` | 8 张 stat-card + 大圆角卡片堆叠，是「Web Dashboard」审美残留，与桌面客户端不符 |
| 无主题能力 | `src/App.vue` | `n-config-provider` 未接 `theme`，无亮/暗切换与系统跟随 |
| 底部栏信息单一 | `src/App.vue` | F-07 版本栏只是纯文本，未承担「状态栏」职责 |
| 工作区切换入口分散 | 各视图 toolbar | Dashboard / RepositoryList / ChangeSet / Health / Runtime 各自维护一份工作区选择器，状态不统一 |
| 窗口状态不记忆 | `src-tauri/tauri.conf.json` | 每次启动固定 1280×800 |

**核心判断**：「原生感」不来自组件本身，而来自**皮肤（tokens + themeOverrides）、密度、骨架（导航/状态栏/面板）、桌面交互（右键/快捷键/命令面板）**。Naive UI 保留，在其上叠加一层薄薄的 Desktop Skin——Naive UI 不是根因，「Naive UI 默认皮肤 + Web Dashboard 布局 + Web 式导航」才是。

## 2. 非目标（明确不做）

- **不自研组件库**：按钮/表格/树/弹窗继续用 Naive UI，不重写、不换库。
- **自定义无边框标题栏：重估完成，暂不实施**（2026-08-27，D-16）：自绘窗口控制按钮/拖拽区/Windows snap 与 macOS 红绿灯适配成本高；当前原生标题栏已足够，套壳感主要来自导航/密度/骨架，已由 D-01~D-15 解决；且自绘存在平台特定 bug 风险。4 期+ 应用成熟度提升后可再评估——它是最后残留的「套壳痕迹」。详见 [tasks-desktop/D-16-splitter-titlebar.md](./tasks-desktop/D-16-splitter-titlebar.md)。
- **不做窗口 vibrancy / 托盘常驻 / 原生菜单 / 文件关联**：装饰性或与本工具定位不符，维持 F-10 讨论结论。
- **不推倒重写视图**：22 个视图作为「面板」嵌入新骨架，逐个收敛而非重写。
- **Dashboard 不做信息流式重设计**（Recent Activity / 动态 feed）：后端无活动流数据源，且 F-01 刚建成，只做视觉收敛。

## 3. 总体设计：Desktop Skin 三层

```
┌────────────────────────────────────────────────┐
│ 第 3 层  骨架组件（6-7 个）                       │
│   AppShell / SideNav / StatusBar / Panel / ...  │
├────────────────────────────────────────────────┤
│ 第 2 层  Naive UI 全局 themeOverrides            │
│   圆角 ≤4px · 密度 small · 字号 13px · 边框收敛    │
├────────────────────────────────────────────────┤
│ 第 1 层  Design Tokens（src/styles/tokens.scss） │
│   语义色 / 间距阶梯 / 字号阶梯 / 等宽字体栈 · 亮暗双套 │
└────────────────────────────────────────────────┘
```

分层语义：Naive UI 是基础控件层，Desktop Skin 是桌面视觉+布局层，业务视图叠加其上——即使未来替换组件库，桌面层架构不受影响。

### 3.1 第 1 层：Design Tokens

新增 `src/styles/tokens.scss`，定义语义化 CSS 变量，**亮/暗两套一期一次到位**（暗色挂在根元素 `[data-theme="dark"]` 上，与 Naive UI darkTheme 联动切换）：

- **颜色**：`--gw-bg-app` / `--gw-bg-panel` / `--gw-bg-hover` / `--gw-border` / `--gw-text` / `--gw-text-dim` / `--gw-accent` / `--gw-success` / `--gw-warning` / `--gw-danger` / `--gw-info`
- **间距**：`--gw-space-1: 4px` ~ `--gw-space-4: 16px`
- **字号**：`--gw-text-xs: 11px` / `--gw-text-sm: 12px` / `--gw-text-md: 13px`（正文默认）/ `--gw-text-lg: 14px`
- **圆角**：`--gw-radius-sm: 2px` / `--gw-radius-md: 4px`
- **字体栈**：正文系统栈（沿用现有）；等宽栈 `--gw-font-mono: ui-monospace, "Cascadia Mono", "JetBrains Mono", Consolas, monospace`，用于路径、分支名、commit hash、日志
- **状态栏/侧栏**：`--gw-statusbar-h: 24px`、`--gw-sidenav-w: 188px` / `--gw-sidenav-w-collapsed: 48px`

存量视图中的 `--el-*` 残留与硬编码色值在二期统一替换为 tokens；一期新增的骨架代码必须全部使用 tokens。

### 3.2 第 2 层：Naive UI themeOverrides

在 `App.vue` 的 `n-config-provider` 上传 `:theme-overrides`，全局生效、22 个视图零改动：

- `common`: `borderRadius: 4px`、`borderRadiusSmall: 2px`、`fontSize: 13px`、主色/边框色与 tokens 同值
- `Button`/`Input`/`Select`/`DataTable` 等：高度收敛到 small 档（28px 左右），表格行高压到 32px
- `Card`/`Dialog`：圆角 4px、内边距收敛
- `theme` 绑定亮/暗：默认跟随系统（Tauri `appWindow.theme()` + `onThemeChanged` 监听）；手动三档（跟随系统/亮/暗）入口放**设置区**，选择持久化到 localStorage

### 3.3 第 3 层：骨架组件

只封装 Naive UI 没有的「外壳」组件（放 `src/components/shell/`）：

| 组件 | 职责 | 引入期 |
|---|---|---|
| `AppShell` | 组合件：SideNav + 内容区 + StatusBar | 一期 |
| `SideNav` | 左侧导航：分组、选中态、折叠（188px ⇄ 48px），折叠状态持久化 | 一期 |
| `StatusBar` | 底部状态栏：分段槽位（可点击），替代 F-07 纯文本 footer | 一期 |
| `Panel` + `PanelHeader` | 带标题栏的面板容器，统一替代各视图手写的 `.section` | 二期 |
| `Toolbar` | 工具行容器，统一各视图工具栏的间距与分组 | 二期 |
| `CommandPalette` | Ctrl/Cmd+K 命令面板：导航 + 高频命令统一入口（基于 `n-modal` + 命令注册表） | 2.5 期 |
| `ContextMenu` | 右键菜单薄封装（基于 `n-dropdown`） | 2.5 期 |

## 4. 布局骨架（IDEA 式三段）

```
┌──────────┬─────────────────────────────────────┐
│          │                                     │
│  SideNav │   内容区（router-view，视图即面板）     │
│  188/48  │                                     │
│          │                                     │
├──────────┴─────────────────────────────────────┤
│ StatusBar：工作区▾ │ 分支 │ ●watcher │ ⏵n 任务 │ v0.1.0 by mantougg │
└─────────────────────────────────────────────────┘
```

### 4.1 SideNav 导航分组（对应现有路由）

全量平铺（已定案）：所有条目图标+文字直出，不做「更多」折叠。

| 分组 | 条目 | 路由 |
|---|---|---|
| 工作区 | 总览 / 变更与批量操作 / 健康检查 | `dashboard` / `changes` / `health` |
| Git | 提交图 / 分支 / Stash / Worktree / Reflog / Change Set / Pipeline / Manifest / 操作日志 | `git-graph` / `branch-manager` / `stash-manager` / `worktree-manager` / `reflog-view` / `change-sets` / `pipeline` / `manifest` / `operation-log` |
| Runtime | Runtime 总览 / 依赖 / 作用域 / 日志 | `runtime-dashboard` / `runtime-dependencies` / `runtime-scope` / `runtime-logs` |
| 设置 | 工作区管理 / JDK 管理 / Maven 设置 | `workspaces` / `jdk-manager` / `maven-settings` |

- 默认落地视图：`dashboard`（已定案，保留为默认首页）。
- Dashboard 工具栏上的导航按钮（`DashboardView.vue:39-86`）全部移除，迁移进 SideNav；Dashboard 只保留工作区操作（扫描/刷新）与数据面板。
- 各视图内的「返回」按钮收进骨架（面包屑或标题栏返回），统一处理 F-02 的导航问题域。
- `diff-viewer` / `conflict-resolver` / `runtime-app-wizard` 作为「任务型页面」不进导航，保持独立路由（已定案），由来源页面带参跳转，骨架提供返回。

### 4.2 StatusBar 槽位

左→右：当前工作区（点击弹切换器，**全局唯一工作区切换入口**）｜当前分支（仅 Git 类视图显示）｜watcher 状态点｜运行中任务数（点击展开 TaskPanel，TaskPanel 唯一入口收编于此）｜（弹性占位）｜`vX.Y.Z by author`（F-07 数据源不变，仍取 `__APP_VERSION__` / `__APP_AUTHOR__`）。

**StatusBar 不变成第二个 Toolbar**：只放状态与高频上下文入口；主题三档切换移入设置区，不常驻状态栏。

工作区切换收编 StatusBar 后，各视图 toolbar 内的工作区选择器全部移除，视图改为响应全局 store 的当前工作区。

### 4.3 窗口与布局记忆

- 窗口尺寸/位置：官方插件 `tauri-plugin-window-state`。
- SideNav 折叠状态、主题选择：localStorage。
- 面板级布局记忆（三期联动面板的 splitter 位置）：localStorage 按视图 key 存储。

## 5. 面板结构布局详述

> 本章是开发的**布局基准**：每个面板的区域划分、尺寸、内容与改造点都以此为准，避免开发过程中跑偏。
> 各区域标注的 `[一期]` `[二期]` `[2.5期]` `[三期]` 表示该区域涉及的改动落在哪一期。

### 5.0 通用规则（所有面板适用）

1. 视图根容器统一为 `height: 100%; display: flex; flex-direction: column`；**滚动只发生在视图内容区，AppShell 自身不滚动**。
2. 间距一律取 `--gw-space-*`，字号一律取 `--gw-text-*`，颜色一律取 `--gw-*` 语义色——禁止新增硬编码色值/像素值。
3. 视图内**不再出现跳转到其他视图的导航按钮**（那是 SideNav 的职责）；toolbar 只保留**作用于当前视图数据的操作按钮**（扫描/刷新/新建/启动等）。
4. 所有「返回」由骨架统一提供，视图内不自绘返回按钮；工作区切换统一走 StatusBar，视图内不再放工作区选择器。
5. 路径、分支名、commit hash、日志内容一律使用 `--gw-font-mono` `[二期]`。

### 5.1 AppShell 整体骨架 `[一期]`

```
┌──────────────────────────────────────────────────────────┐
│ 原生标题栏（OS 提供，一期/二期不做自定义）                    │
├──────────┬───────────────────────────────────────────────┤
│          │                                               │
│  SideNav │   内容区 <router-view>                          │
│  188px   │   padding: 12px 16px（--gw-space-3/4）          │
│  ⇄ 48px  │   overflow-y: auto（滚动归视图）                 │
│          │                                               │
├──────────┴───────────────────────────────────────────────┤
│ StatusBar（24px，--gw-statusbar-h，上边框 1px --gw-border）  │
└──────────────────────────────────────────────────────────┘
```

- SideNav 与内容区之间只有 1px 边框（`--gw-border`），**不留空隙、不加阴影**——客户端面板是「贴」在一起的。
- 内容区背景 `--gw-bg-app`，面板（Panel）背景 `--gw-bg-panel`，两层背景色差是「面板化」观感的关键。
- 最小窗口 900×600（沿用 tauri.conf）；SideNav 在空间不足时手动折叠，不做自动响应式（桌面客户端语义）。

### 5.2 SideNav `[一期]`

```
┌──────────────┐
│ GitWorkspace │  产品名区：13px 半粗，高 40px，下边框 1px
├──────────────┤
│ 工作区        │  分组标题：11px（--gw-text-xs）--gw-text-dim
│ ▣ 总览        │  条目：16px 图标 + 13px 文字，行高 32px
│ ▣ 变更与批量操作│  选中态：左侧 2px --gw-accent 指示条 + --gw-bg-hover 背景
│ ▣ 健康检查     │  hover：--gw-bg-hover（比选中态无指示条）
│ Git          │
│ ▣ 提交图      │  …（Git 组 9 项全量平铺）
│ …            │
│ Runtime      │  …（4 项）
│ 设置         │  …（3 项）
├──────────────┤
│ ◀ 折叠        │  底部折叠按钮，188px ⇄ 48px
└──────────────┘
```

- 宽度 188px（评审修订：200px 在 Git 组 9 项下视觉重量过大）；折叠态 48px。
- 折叠态只显示图标，label 用 tooltip 呈现；折叠状态写 localStorage。
- 当前路由对应条目高亮；任务型页面（Diff 等）不高亮任何条目，但保持其来源分组可见。
- 分组顺序固定：工作区 → Git → Runtime → 设置。

### 5.3 StatusBar `[一期]`

```
│ 工作区: default ▾ │ main │ ● │ ⏵ 2 个任务 │        (弹性占位)        │ v0.1.0 by mantougg │
```

- 高度 24px，字号 11px（`--gw-text-xs`），槽位间用 1px 竖分隔线或 8px 间距。
- 可点击槽位（工作区 / 任务数）hover 显示 `--gw-bg-hover` 背景。
- 槽位语义：
  - **工作区**：显示当前工作区名，点击弹出切换器（列出全部工作区 + 「管理工作区…」入口）——全局唯一切换入口。
  - **分支**：仅在 Git 类视图（变更/提交图/分支等）显示当前仓库分支；无上下文时隐藏槽位。
  - **watcher**：绿点=监听中，灰点=未启动；hover tooltip 显示监听仓库数。
  - **任务数**：`⏵ n 个任务`，点击展开/收起 TaskPanel；无任务时显示「无任务」且不可点击。
  - **版本**：F-07 数据源不变，永远在最右侧。
- 主题三档切换**不在状态栏**，入口放设置区（评审修订：状态栏只放状态，不变成第二个 Toolbar）。

### 5.4 Dashboard（总览，默认首页）

```
┌ Toolbar ────────────────────────────────────────┐
│ [扫描仓库] [刷新]              （其余按钮全部移除） │
├ 摘要行（高密度，无卡片边框）──────────────────────┤
│ 仓库 32 · 干净 20 · 有变更 8 · 冲突 2 · ↑3 ↓5 …  │
├ Panel: 状态分布 ────────────────────────────────┤
├ Panel: 提交热力图 ──────────────────────────────┤
├ Panel: 健康检查 ────────────────────────────────┤
├ Panel: 我的应用 ────────────────────────────────┤
├ Panel: 分组视图 ────────────────────────────────┤
├ Panel: 快捷操作 ────────────────────────────────┤
└─────────────────────────────────────────────────┘
```

- `[一期]` 移除 toolbar 右侧全部导航按钮（健康检查/Change Set/Pipeline/操作日志/JDK/Runtime/变更与批量操作，共 7 个）与「添加工作区」「工作区管理」（迁移：添加工作区 → SideNav 设置组的工作区管理页内；工作区选择器移除，走 StatusBar）。
- `[一期]` 保留：扫描仓库、刷新（作用于当前视图数据的操作）。
- `[二期]` **卡片墙（8 张 stat-card）收敛为一条高密度摘要行**：数字+标签平铺、无卡片边框、无大圆角；**保留现有卡片的点击跳转语义**（点击指标跳转变更视图并预填 `@status:xxx` 选择器，见 `DashboardView.vue::openCard`）。
- `[二期]` 各 `.section` 替换为 `Panel` 组件；`--el-*` 变量（`DashboardView.vue:544` 等）全部替换为 `--gw-*`。
- 明确不做：信息流式 Overview / Recent Activity（后端无活动流数据源，评审结论已对齐）。

### 5.5 变更与批量操作（RepositoryList）

```
┌ Toolbar ────────────────────────────────────────┐
│ [扫描仓库] [启动监听]              [搜索文件或仓库…] │
├──────────────────────────┬──────────────────────┤
│ tree-pane                │ diff-pane            │
│ ┌ stats-bar ───────────┐ │ ┌ diff-pane-header ┐ │
│ │ 共n仓库│n有变更│…      │ │ │ 文件路径(mono)   │ │
│ ├ scan-progress ───────┤ │ ├ diff-pane-body   ┤ │
│ │ (扫描时显示进度条)     │ │ │ 内联 diff 预览   │ │
│ ├ 变更树 ──────────────┤ │ └──────────────────┘ │
│ │ (仓库→文件 复选树)    │ │                      │
│ └──────────────────────┘ │                      │
├──────────────────────────┴──────────────────────┤
│ commit-panel（提交信息输入 + 批量操作按钮）           │
└─────────────────────────────────────────────────┘
```

- 现有结构（`main-body > tree-pane + diff-pane` + 底部 `commit-panel`）**保持不变**，这是全应用最接近 Git 客户端的视图。
- `[一期]` 移除：返回按钮、添加工作区、健康检查/Change Set/Pipeline/Manifest/操作日志 5 个导航按钮、「任务 (n)」按钮（收编 StatusBar）、工作区选择器（收编 StatusBar）。
- `[一期]` 保留：扫描仓库、启动/停止监听、搜索框；「日志」按钮（打开日志目录，属当前视图操作）保留。
- `[2.5期]` 变更树/仓库节点接入 `ContextMenu`（Fetch/Pull/Push/提交/在 IDE 打开等）。
- `[三期]` 此视图演化为「仓库树 + 提交图 + diff」三栏联动（见 §6 三期）。

### 5.6 提交图（GitGraph）

```
┌ 视图头 ─────────────────────────────────────────┐
│ repo-path（mono）        [刷新] [Reflog]         │
├ 分支条 branch-bar ──────────────────────────────┤
│ [main] [develop] [origin/main] …（前 10 个分支）  │
├ 冲突横幅 conflict-bar（仅冲突时出现）──────────────┤
├─────────────────────────────────────────────────┤
│ graph-body（CommitGraph 提交图，虚拟滚动）          │
└─────────────────────────────────────────────────┘
```

- `[一期]` 返回按钮移除（骨架提供）；repo-path 区域保留作为视图标题。
- `[二期]` repo-path、分支名、hash 接等宽字体；conflict-bar 色值 token 化。
- `[2.5期]` 提交节点接入 `ContextMenu`（Checkout / Reset / Cherry-pick / Copy hash 等，复用现有 `@action` 分发）。

### 5.7 Runtime 总览（RuntimeDashboard）

```
┌ Toolbar ────────────────────────────────────────┐
│ [刷新] [解析依赖] [新建应用]      [全部启动] [全部停止] │
├ RuntimeErrorAlert（仅有错误时显示）────────────────┤
├ 统计摘要行（应用配置/运行中/…，同 §5.4 摘要行样式）──┤
├ Panel: 应用列表（表格）────────────────────────────┤
└─────────────────────────────────────────────────┘
```

- `[一期]` 移除导航类按钮：依赖映射 / Scope / 日志（进 SideNav Runtime 组）；返回、工作区选择器移除。
- `[一期]` 保留操作类：刷新、解析依赖、新建应用（跳 wizard 属「发起任务」而非导航，保留）、全部启动、全部停止。
- `[二期]` stat-card 与 Dashboard 同步收敛为摘要行。
- Runtime 其余子页（依赖/Scope/日志）同模式：返回移除，操作保留。

### 5.8 任务型页面（DiffViewer / ConflictResolver / RuntimeAppWizard）

```
┌ 骨架返回 + 视图标题栏 ───────────────────────────┐
│ ← 返回 │ repo-path（mono）│ 页面自有操作…          │
├─────────────────────────────────────────────────┤
│ 内容区（全宽）                                    │
└─────────────────────────────────────────────────┘
```

- 保持独立路由（已定案），不进 SideNav；由来源视图带参跳转，骨架返回回到来源页。
- `[一期]` 视图内自绘的返回按钮移除，改由骨架统一提供（F-02 问题域集中处理点）。
- 页面自有操作（Diff 的 source/mode 切换、AI Review；解决器的刷新/中止；向导的步骤操作）保留在视图标题栏/内容区。
- `[二期]` DiffViewer 的 diff 内容区、冲突解决器的代码区接等宽字体。

### 5.9 其余列表/工具页（统一模式）

Change Set / 健康检查 / Pipeline / Manifest / 操作日志 / 分支 / Stash / Worktree / Reflog / 工作区管理 / JDK 管理 / Maven 设置 / Runtime 子页，统一收敛为一个模式：

```
┌ 视图标题（骨架面包屑/标题）───────────────────────┐
├ Toolbar ────────────────────────────────────────┤
│ [新建/刷新/…当前视图操作]                          │
├─────────────────────────────────────────────────┤
│ 内容 Panel（列表 / 表格 / 表单）                    │
└─────────────────────────────────────────────────┘
```

- `[一期]` 移除：返回按钮、工作区选择器、「任务 (n)」按钮、指向其他视图的导航按钮。
- `[二期]` 内容区 `.section`/裸容器逐个替换为 `Panel`，工具行替换为 `Toolbar`。
- 各页差异只在内容 Panel 内部，外壳完全一致——这是「统一模式」的意义：新增页面不允许自创外壳。

### 5.10 TaskPanel `[一期]`

- 保持现有的右下角悬浮面板形态与业务逻辑不变。
- 唯一变化：唤起入口从各视图 toolbar 的「任务 (n)」按钮收编为 StatusBar 任务槽位；视图内任务按钮全部移除。

### 5.11 Command Palette `[2.5期]`

```
        ┌─────────────────────────────────────┐
        │ > 输入命令或搜索视图…                  │
        ├─────────────────────────────────────┤
        │ 切换工作区…                            │
        │ 扫描仓库                               │
        │ Fetch 全部仓库                         │
        │ 打开: 提交图 / 分支 / Change Set / …    │
        │ 新建 Change Set                       │
        │ 运行健康检查                            │
        └─────────────────────────────────────┘
```

- `Ctrl/Cmd+K` 唤起（WebView 内自实现 keydown，不依赖系统菜单）；居中顶部浮层，模糊搜索 + 键盘上下选择 + Enter 执行。
- 命令注册表集中定义（`src/commands/`）：每条命令 = id + 标题 + 分组 + 执行函数（导航 push 或调用现有 store/api 方法）。**只编排已有能力，不新增业务逻辑**。
- 它同时是导航体系与快捷键体系的承载：快捷键只做「命令 id → 按键」的映射表，不各自实现。

## 6. 分期实施

### 一期：Desktop Shell（布局骨架 + 主题系统）

1. `src/styles/tokens.scss`：语义 tokens，**亮/暗双套一次到位**（暗色挂 `[data-theme="dark"]`）。
2. 主题机制：`n-config-provider` 接 `theme`（light / darkTheme），默认跟随系统（Tauri `appWindow.theme()` + `onThemeChanged`）；三档切换入口放设置区，localStorage 持久化。
3. `src/components/shell/`：`AppShell` / `SideNav` / `StatusBar`。
4. `App.vue` 改为 `AppShell > SideNav + router-view + StatusBar`；TaskPanel 入口收编状态栏。
5. router 按 §4.1 补 meta（分组 / 标题 / 图标 / 是否进导航）；默认路由 `dashboard` 不变。
6. 导航清理（按 §5 各面板的 `[一期]` 项）：移除各视图导航按钮 / 返回按钮 / 任务按钮 / 工作区选择器；返回逻辑统一进骨架（**回归验证 F-02 场景**）；各视图改为响应全局当前工作区。
7. `tauri-plugin-window-state` 接入（窗口尺寸/位置记忆）。
8. AGENTS.md 新增 tokens 使用约定（见 §7）。

### 二期：Desktop Visual System（视觉密度收敛）

1. Naive UI 组件级 themeOverrides（§3.2 的 Button/Table/Card 等）。
2. 全局替换 `--el-*` 残留与硬编码色值为 tokens（逐视图）。
3. 路径/分支名/hash/日志/diff 接入 `--gw-font-mono`。
4. `Panel` / `PanelHeader` / `Toolbar` 组件抽取，各视图 `.section` / `.toolbar` 逐个替换。
5. **Dashboard / Runtime 的 stat-card 墙收敛为高密度摘要行**（§5.4/§5.7，保留点击跳转语义）；其余自定义视觉件（健康分、热力图）按 §5 布局图收敛。

### 2.5 期：Desktop Interaction（桌面交互，评审新增）

1. `CommandPalette`：命令注册表 + Ctrl/Cmd+K 唤起（§5.11）。
2. `ContextMenu`：变更树仓库/文件节点、提交图节点右键菜单（§5.5/§5.6）。
3. 键盘快捷键体系：导航 `Ctrl/Cmd+1..9`、刷新、Command Palette——统一走命令注册表的按键映射。
4. 多选/拖拽行为评估（变更树文件多选已有复选；拖拽仅在有明确场景时做，不提前实现）。

### 三期：Git Client Experience（可选，独立评估）— 已完成（2026-08-27/28）

1. 变更视图三栏联动：仓库树 + 提交图 + diff（参照 Fork/IDEA Git 工具窗口）→ D-15。
2. 面板 splitter 位置记忆（localStorage 按视图 key）→ D-16。
3. 自定义无边框标题栏重估 → D-16，**结论：暂不实施**（见 §2）。

## 7. 工程约束

- **新增 AGENTS.md 约定**（一期落地时同步写入）：新 UI 一律使用 tokens 变量，禁止硬编码色值/像素间距；新面板一律使用 `Panel`/`Toolbar` 骨架组件；正文字号/圆角/组件密度以 themeOverrides 为准，视图内不单独覆盖；新页面外壳遵循 §5.9 统一模式。
- **工作区切换全局唯一入口为 StatusBar**；视图内不得再出现工作区选择器。
- **命令与快捷键统一走命令注册表**（2.5 期起），禁止在视图内各自绑定快捷键。
- **平台规范**：窗口状态、主题监听等 Tauri API 使用遵守根 AGENTS.md「平台兼容性开发规范」。
- **最小改动**：一期不动任何视图的业务逻辑与 IPC 调用，纯壳层 + 导航清理。
- **F-07 兼容**：版本栏数据源与注入机制（`vite.config.ts` define）不变，仅展示位置移入 StatusBar；AGENTS.md 的 F-07 条目届时更新为状态栏最右槽位。

## 8. 验收标准

### 一期

- [ ] 所有导航视图经 SideNav 可达，Dashboard 不再承担导航中转；启动默认落地 Dashboard
- [ ] F-02 场景回归通过：Change Set 等任务型页面可正常返回来源页
- [ ] 状态栏：工作区切换可用（全局唯一入口）、任务数可唤起 TaskPanel、版本号在最右
- [ ] 主题跟随系统实时切换；设置区三档可手动覆盖并持久化
- [ ] 窗口尺寸/位置重启后恢复
- [ ] `pnpm build`（含 vue-tsc）通过

### 二期

- [ ] Dashboard / Runtime 无 stat-card 卡片墙，摘要行保留点击跳转语义
- [ ] 全局 grep 无 `--el-` 变量残留，无色值硬编码新增
- [ ] 亮/暗两套主题下逐视图目检无破版（对照 §5 布局图）
- [ ] 默认密度下 1280×800 首屏信息密度明显提升（对照改造前截图）

### 2.5 期

- [ ] Ctrl/Cmd+K 唤起命令面板，可搜索并执行导航类与高频操作类命令
- [ ] 变更树仓库节点、提交图提交节点有右键菜单且命令可用
- [ ] 快捷键全部经命令注册表映射，视图内无独立 keydown 绑定

### 三期（已落地）

- 三栏联动（D-15）：树单选 → 中栏提交图；提交点选/右键 → DiffViewer 显示该提交变更；文件双击 → 右栏内联 diff；批量操作流不受影响。
- splitter 位置记忆（D-16）：diff-pane 宽度 `gw-diff-width` localStorage 持久化，拖拽结束保存、重启恢复。
- 标题栏重估（D-16）：**暂不实施**，结论与理由见 §2 与 D-16 任务文档。

## 9. 风险

| 风险 | 缓解 |
|---|---|
| 导航模型变更回归 F-02（Change Set 返回） | 一期验收强制回归该场景；返回逻辑集中到骨架单点处理 |
| 工作区选择器收编 StatusBar 影响全部视图的数据加载触发 | 各视图统一改为 watch 全局 store 当前工作区；逐视图回归加载/扫描行为 |
| 摘要行改造丢失卡片的跳转能力 | 摘要行每个指标保留 `@status:xxx` 预填跳转（现有 `openCard` 语义），二期验收强制覆盖 |
| themeOverrides 与视图内联样式冲突 | 二期先替换硬编码再调覆盖；以截图对比验收 |
| 骨架组件过度抽象 | 只封装 §3.3 列出的外壳组件，禁止提前抽象业务组件 |
| Command Palette 演变成「第二套业务逻辑」 | 命令注册表只允许编排已有 store/api 能力；评审时拒绝新增业务逻辑的命令 |
| 亮暗双套 tokens 一期工作量大 | 一期只保证骨架 + tokens 双套可用；存量视图的暗色观感允许二期随硬编码替换逐步收敛，一期验收不卡存量视图暗色细节 |
