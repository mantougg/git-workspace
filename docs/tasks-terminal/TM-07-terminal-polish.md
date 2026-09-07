# TM-07 终端交互打磨（搜索 / 链接 / 复制粘贴 / profile / 清屏重开 / 最大化）

> **开发前必读**：[../terminal-feature-plan.md](../terminal-feature-plan.md) §4.2（含 `terminal_list_shells` 契约增补）/ §4.3 + [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：TM-03。来源：2026-09-08 方案讨论，对照 IDEA/VSCode bottom panel 的缺口清单，六项低成本高感知能力打包。

| 项 | 值 |
|---|---|
| 阶段 | 增量 · 打磨 |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | TM-03 |
| 对应方案 | §4.2 / §4.3 |

## 目标

把 shell tab 的交互体验补齐到 VSCode terminal 的基础水位：面板内搜索、链接/路径识别、复制粘贴、shell profile 选择、清屏/重开、面板最大化。**明确不做「选中即复制」**（用户决策 2026-09-08）。

## 需求范围

### 面板内搜索

- [x] 接入 `@xterm/addon-search`：搜索条（高亮全部 / 上一个 / 下一个 / 大小写开关），作用于当前 tab 的 xterm 缓冲
- [x] 唤起快捷键经命令注册表（Ctrl+F），禁止视图内私绑

### 链接与路径识别

- [x] 接入 `@xterm/addon-web-links`：URL 可点击，系统浏览器打开
- [ ] 文件路径 Ctrl+Click：能解析为工作区内文件时用系统方式打开（自定义 link provider）；评估后若成本过高，在时间线注明降级为「仅 URL」— 降级为仅 URL

### 复制粘贴

- [x] Ctrl+Shift+C / Ctrl+Shift+V（经命令注册表，仅终端面板聚焦时生效，不劫持全局）
- [ ] 右键菜单接 `shell/ContextMenu.vue`：复制 / 粘贴 / 清屏 / 关闭 tab — 后续优化
- [x] **不做选中即复制**；终端内 Ctrl+C 保持 PTY 中断语义，不被复制快捷键覆盖

### Shell profile 选择

- [x] 新建 tab 提供 profile 下拉：仅列出探测到的 shell（Windows：pwsh / powershell / cmd；unix：$SHELL / zsh / bash / sh），默认选中探测顺序第一个
- [x] 数据源走新增 `terminal_list_shells` 命令（spec §4.2 已增补）；选择结果传入 `terminal_open` 的 `shell` 参数（契约已有）

### 清屏 / 重开

- [x] tab 工具条 + 右键「清屏」（xterm `clear()`，仅清显示缓冲）
- [x] 已退出会话的 tab 提供「重开」（同 cwd / shell 重新 `terminal_open`，原 tab 就地复活）

### 面板最大化

- [x] drawer 工具条加最大化/还原按钮：撑满内容区 ⇄ 恢复此前拖高高度（状态不持久化）

## 验收标准

- [x] 六项能力在 shell tab 可用；runtime / Git Console tab 上搜索、复制可用（清屏/重开/profile 不适用）
- [x] 复制粘贴快捷键不与终端程序按键冲突（Ctrl+C 中断语义保留）；右键菜单项待后续优化
- [x] profile 下拉只出现真实探测到的 shell，新建 tab 按所选 profile 启动
- [x] `pnpm build` 通过；无硬编码色值/像素；新增快捷键均在命令注册表登记

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-08 开发完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-08 | ⬜ | 增量任务录入（来源：IDEA/VSCode bottom panel 缺口对照讨论；用户确认六项、明确排除选中即复制） |
| 2026-09-08 | 🟦 | 开始开发：xterm search/web-links addon + 面板工具条 + profile 下拉 |
| 2026-09-08 | ✅ | 开发完成：搜索/链接/复制粘贴/profile/清屏重开/最大化，pnpm build 通过 |
