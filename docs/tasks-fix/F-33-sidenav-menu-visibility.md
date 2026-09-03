# F-33 左侧菜单可见性配置（低频入口收纳）

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-03 用户反馈（第 1 项） |
| 关联任务 | D-XX Desktop Skin（SideNav 骨架）、T-31 命令注册表 |

## 问题描述

左侧菜单共 29 个导航项分 4 组（工作区 3 / Git 12 / Runtime 5 / 设置 7+），其中
符号、仓库工具、自动化、Reflog、Pipeline、多服务环境等入口使用频率很低，
长期占位。用户希望低频菜单可以收起来，或提供一个设置入口配置展示哪些菜单。

## 定位线索

- `src/components/shell/SideNav.vue`：导航完全由 router meta 驱动
  （`navGroups` 从 `router.getRoutes()` 的 `meta.group` 分组）；已有**整栏折叠**
  （`gw-sidenav-collapsed`，localStorage），但无逐项可见性配置。
- `src/router/index.ts`：`meta.group / title / nav` 是菜单数据源；
  任务型页面 `nav: false` 不进菜单。
- `src/commands/registry.ts`：命令面板（Ctrl+K）的 `nav:` 命令同样从
  `router.getRoutes()` 生成，**不经过 SideNav**——菜单隐藏不影响命令面板直达。
- Desktop Skin 约束（AGENTS.md）：新 UI 一律用 tokens 变量，导航按钮统一在
  SideNav，禁止视图内自绘导航。

## 方案（选定：菜单可见性设置）

1. **设置入口**：SideNav 底部（折叠按钮上方）加一个「菜单配置」按钮，
   打开 `n-modal` 设置弹窗；不新增路由页面（避免「配置菜单的菜单」又占一格）。
2. **配置 UI**：弹窗内按分组（工作区 / Git / Runtime / 设置）列出全部导航项
   + `n-checkbox`；每组带「全选/清空」；底部「恢复默认」。
3. **持久化**：localStorage `gw-sidenav-hidden-nav`，存**隐藏项的 route name
   列表**（黑名单式：新增菜单默认可见，不会被旧配置误伤）。
   首次无存储时按用户点名写入默认隐藏集：
   `symbol-search / repo-tools / automation / reflog-view / pipeline /
   runtime-environments`。
4. **渲染过滤**：`SideNav.vue` 的 `navGroups` computed 过滤隐藏项；
   折叠态（图标模式）同样过滤；整栏折叠逻辑不动。
5. **边界**：
   - 隐藏仅影响 SideNav 渲染；直接 URL 路由与命令面板 `nav:` 命令照常可达。
   - 当前正位于被隐藏页面时**不强制跳转**，仅菜单不再高亮/显示该项
     （`isActive` 对隐藏项照常生效）。
   - 全部隐藏某组时不渲染空分组标签。

### 备选（未采纳）

- 分组级折叠（组标题点击收起）：实现更简单，但粒度粗——用户点名的是
  6 个具体低频项，分散在 Git/Runtime 两个组里，组折叠解决不了。
- 塞进「关于」页：设置入口太深，日常调整不便。

## 修复范围

- [x] SideNav 增加菜单配置入口 + 可见性过滤（含折叠态）
- [x] 菜单可见性设置弹窗（分组 checkbox + 恢复默认）
- [x] localStorage 持久化（黑名单式）+ 首次默认隐藏集
- [x] 隐藏页面的路由/命令面板可达性验证
- [x] Desktop Skin tokens 合规（弹窗用 Naive UI 组件，不自绘样式硬编码）

## 验收标准

- [x] 默认安装后，用户点名的 6 个低频菜单不显示，其余 23 项正常
      （DEFAULT_HIDDEN_NAV 精确等于用户点名集合，代码审查确认）
- [x] 设置弹窗可任意勾选/取消，刷新后配置保留
      （黑名单写 localStorage，loadHiddenNav 容错解析）
- [x] 被隐藏页面仍可通过 URL 与 Ctrl+K 命令面板直达
      （过滤只作用于 navGroups computed；registry.ts 的 nav: 命令独立从
      router 生成，不读 localStorage 黑名单——代码路径分离确认）
- [x] 整栏折叠、选中高亮、title 提示等既有行为不回退
      （折叠/高亮逻辑未改动，仅 navGroups 数据源过滤）
- [x] `pnpm build`（含 vue-tsc）通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-03 修复完成，构建通过

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-03 | ⬜ | 用户反馈录入：低频菜单（符号/仓库工具/自动化/Reflog/Pipeline/多服务环境）需可配置收纳 |
| 2026-09-03 | 🟦 | 开始修复：SideNav 增加菜单配置弹窗（黑名单持久化） |
| 2026-09-03 | ✅ | 修复完成：SideNav「菜单配置」按钮 + n-modal 弹窗（分组 checkbox / 全部显示隐藏 / 恢复默认），localStorage `gw-sidenav-hidden-nav` 黑名单式持久化，首次写入用户点名的 6 项默认隐藏；navGroups 过滤仅影响渲染（折叠态共用），URL/命令面板直达不受影响。验证：`pnpm build`（vue-tsc + vite）通过；默认隐藏集与代码路径分离逐项代码审查确认。注：GitNexus MCP 工具本会话不可用，影响面以全量通读 SideNav/registry/router 消费方替代 |
