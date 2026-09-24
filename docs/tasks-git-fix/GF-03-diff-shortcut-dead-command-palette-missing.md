# GF-03 Ctrl+Shift+D 死键 + Diff/冲突解决器从命令面板消失

> 状态：⬜ 未开始
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

## 修复范围 checklist

- [ ] 1. 在 `registry.ts` 为 diff-viewer / conflict-resolver 单独注册命令（如 `git:diff` / `git:open-conflict-resolver`），不依赖路由自动生成。
- [ ] 2. `Ctrl+Shift+D` 恢复生效；冲突解决器补一个快捷键（如 `Ctrl+Shift+C`，避开已占用键位）。
- [ ] 3. 命令面板按上述入口可搜到「Diff」「冲突解决」。

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
