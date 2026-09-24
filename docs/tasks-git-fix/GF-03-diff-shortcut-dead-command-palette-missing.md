# GF-03 Ctrl+Shift+D 死键 + Diff/冲突解决器从命令面板消失

> 状态：✅ 已完成
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

`Ctrl+Shift+D` 注册为打开 Diff 视图的快捷键，但**按下毫无反应**；同时在命令面板里
搜「Diff」「冲突」也找不到入口。根因是导航命令由路由表自动生成，而 diff-viewer /
conflict-resolver 两个路由标了 `nav: false`（不进 SideNav 的任务型页面），被注册表
过滤掉，导致快捷键绑到了一个从未注册的命令上。

## 定位线索（证据）

- 快捷键映射：`src/commands/shortcuts.ts:24`（`"nav:diff-viewer": ["Ctrl+Shift+D"]`）
- 路由标记：`src/router/index.ts:190-192`（`name: "diff-viewer"`，`meta: { group: "无", title: "Diff", nav: false }`）；`conflict-resolver` 同路段落同样 `nav: false`
- 过滤逻辑：`src/commands/registry.ts:58-59`（`.filter((r) => r.meta.nav !== false && r.name)`）——`nav: false` 的路由永远不会生成 `nav:<name>` 命令
- 冲突解决器不但无快捷键，命令面板也完全不可达（只能从仓库头像/冲突按钮进）。

**复现结论（2026-09-24）**：全部成立。`nav:diff-viewer` 全 `src/` 仅 shortcuts.ts 一处引用（死的绑定）；两路由 `nav: false` 确认。补充事实：`Ctrl+Shift+C` 已被 `terminal:copy` 占用（shortcuts.ts:29），任务文档建议的冲突解决器快捷键 `Ctrl+Shift+C` **不可用**，改用 `Ctrl+Shift+X`（全表无冲突）。

## 修复范围 checklist

- [x] 1. 在 `registry.ts` 为 diff-viewer / conflict-resolver 单独注册命令（如 `git:diff` / `git:open-conflict-resolver`），不依赖路由自动生成。
- [x] 2. `Ctrl+Shift+D` 恢复生效；冲突解决器补一个快捷键（如 `Ctrl+Shift+C`，避开已占用键位）。
- [x] 3. 命令面板按上述入口可搜到「Diff」「冲突解决」。

## 实现说明（2026-09-24）

- `registry.ts` `getGitCommands` 显式注册 `git:diff`（Diff 视图）与 `git:open-conflict-resolver`（冲突解决器）。`ctx` 解构补 `workspaceStore`。
- `git:diff` 无参 push `diff-viewer`：DiffViewer `onMounted` 走 `resolveCurrentRepo`（query.repo → 全局当前仓库 → 工作区首仓库兜底，F-14/F-17），无参直达有兜底。
- `git:open-conflict-resolver` 与 RepositoryList「冲突」入口同参（`workspace` + `name`，队列模式扫全部冲突仓库）——ConflictResolver 无参直达会 warning 并回变更页；无工作区上下文时回变更页。
- `shortcuts.ts`：`"nav:diff-viewer": ["Ctrl+Shift+D"]` → `"git:diff": ["Ctrl+Shift+D"]`；新增 `"git:open-conflict-resolver": ["Ctrl+Shift+X"]`。
- 快捷键唯一性：人工核对 SHORTCUT_MAP 全表（注册表无运行时唯一性校验），Ctrl+Shift+X 无占用；Ctrl+Shift+D 原绑定对象（不存在的 nav:diff-viewer）已移除。

## 不做（范围控制）

- 不把这两个路由加进 SideNav（nav:false 的设计意图保留）。
- 不动 shortcuts.ts 其他映射。

## 验收标准

1. 按 `Ctrl+Shift+D` 能打开 Diff 视图；命令面板搜「diff」可找到并执行。
2. 冲突解决器可经命令面板/快捷键打开。
3. `pnpm build` 通过；既有快捷键无冲突（keys 唯一性检查）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（静态证据见上）。待复现与修复。 |
| 2026-09-24 | 复现成立（nav:diff-viewer 仅 shortcuts.ts 一处死引用；两路由 nav:false 确认；Ctrl+Shift+C 已被 terminal:copy 占用→冲突解决器改 Ctrl+Shift+X）。修复：registry.ts 显式注册 git:diff / git:open-conflict-resolver（后者带 workspace 队列模式参数，与 RepositoryList 入口同参）；shortcuts.ts 重绑 Ctrl+Shift+D 到 git:diff、新增 Ctrl+Shift+X。验证：`pnpm build`（vue-tsc + vite）通过。状态 → ✅。 |
