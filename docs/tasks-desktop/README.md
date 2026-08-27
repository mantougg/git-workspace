# GitWorkspace 桌面化改造任务总览

> 来源：[../desktop-skin-plan.md](../desktop-skin-plan.md)（F-10 桌面化 UI 改造方案，含评审二次修订）。
> 拆分原则：按方案 §6 分期拆为 **D-XX 任务**，每个任务一个独立文档（同目录下 `D-XX-<slug>.md`），可独立跟踪进度与验收。
> **设计 spec 与执行状态分离**：布局/视觉/交互基准以 `../desktop-skin-plan.md` 为准（尤其 §5 面板结构布局详述，开发时对照执行，防止跑偏）；执行状态以本索引 + 各任务文档「进度」为准。

---

## 状态图例

| 图标 | 状态 |
|---|---|
| ⬜ | 未开始 |
| 🟦 | 进行中 |
| ✅ | 已完成 |
| ⏸️ | 暂停 / 阻塞 |

## 总体进度

- 任务总数：**16**
- 已完成：**0** · 进行中：**0** · 未开始：**16**
- 完成度：**0 / 16（0%）**

---

## 阶段与任务索引

### 一期 · Desktop Shell（布局骨架 + 主题系统，6 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| D-01 | Design Tokens（tokens.scss 亮暗双套） | P0 | ⬜ | — | [D-01-design-tokens.md](./D-01-design-tokens.md) |
| D-02 | 主题机制（darkTheme 绑定 / 系统跟随 / 三档持久化） | P0 | ⬜ | D-01 | [D-02-theme-system.md](./D-02-theme-system.md) |
| D-03 | 骨架组件（AppShell / SideNav / StatusBar） | P0 | ⬜ | D-01 | [D-03-shell-components.md](./D-03-shell-components.md) |
| D-04 | App.vue 壳层改造 + router meta + TaskPanel 收编 | P0 | ⬜ | D-03 | [D-04-app-shell-integration.md](./D-04-app-shell-integration.md) |
| D-05 | 导航清理 + 工作区切换收编（含 F-02 回归） | P0 | ⬜ | D-04 | [D-05-nav-cleanup.md](./D-05-nav-cleanup.md) |
| D-06 | 窗口状态记忆 + AGENTS.md 约定落地 | P1 | ⬜ | D-04 | [D-06-window-state-conventions.md](./D-06-window-state-conventions.md) |

### 二期 · Desktop Visual System（视觉密度收敛，5 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| D-07 | Naive UI 组件级 themeOverrides | P1 | ⬜ | D-01 | [D-07-theme-overrides.md](./D-07-theme-overrides.md) |
| D-08 | `--el-*` 残留与硬编码色值全局替换 | P1 | ⬜ | D-01 | [D-08-token-migration.md](./D-08-token-migration.md) |
| D-09 | 等宽字体栈接入（路径/分支名/hash/日志/diff） | P2 | ⬜ | D-08 | [D-09-mono-font.md](./D-09-mono-font.md) |
| D-10 | Panel / PanelHeader / Toolbar 抽取与逐视图替换 | P1 | ⬜ | D-07 | [D-10-panel-toolbar.md](./D-10-panel-toolbar.md) |
| D-11 | Dashboard / Runtime 摘要行收敛 + 自定义视觉件 | P1 | ⬜ | D-10 | [D-11-summary-strip.md](./D-11-summary-strip.md) |

### 2.5 期 · Desktop Interaction（桌面交互，3 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| D-12 | 命令注册表 + Command Palette（Ctrl/Cmd+K） | P1 | ⬜ | D-04 | [D-12-command-palette.md](./D-12-command-palette.md) |
| D-13 | ContextMenu（变更树 / 提交图右键菜单） | P2 | ⬜ | D-12 | [D-13-context-menu.md](./D-13-context-menu.md) |
| D-14 | 键盘快捷键体系（命令注册表按键映射） | P2 | ⬜ | D-12 | [D-14-keyboard-shortcuts.md](./D-14-keyboard-shortcuts.md) |

### 三期 · Git Client Experience（可选，独立评估，2 个）

| 编号 | 任务 | 优先级 | 状态 | 依赖 | 文档 |
|---|---|---|---|---|---|
| D-15 | 变更视图三栏联动（仓库树 + 提交图 + diff） | P2 | ⬜ | D-10 | [D-15-three-pane-changes.md](./D-15-three-pane-changes.md) |
| D-16 | splitter 位置记忆 + 自定义标题栏重估 | P3 | ⬜ | D-15 | [D-16-splitter-titlebar.md](./D-16-splitter-titlebar.md) |

---

## 依赖链

```
D-01 Tokens ──► D-02 主题机制
           ──► D-03 骨架组件 ──► D-04 壳层改造 ──► D-05 导航清理
                                            ──► D-06 窗口状态/约定
           ──► D-07 themeOverrides ──► D-10 Panel/Toolbar ──► D-11 摘要行
           ──► D-08 token 迁移 ──► D-09 等宽字体
D-04 ──► D-12 命令面板 ──► D-13 ContextMenu / D-14 快捷键
D-10 ──► D-15 三栏联动 ──► D-16 splitter 记忆 + 标题栏重估
```

- **一期按 D-01 → D-06 顺序执行**（D-02/D-03 可在 D-01 后并行，D-06 与 D-05 可并行）。
- 二期 D-07 与 D-08 可并行；2.5 期在一期完成后即可插入，不必等二期结束。
- 三期为可选项，启动前需重新评估并细化验收标准。

---

## 维护规范

1. 更新任务状态时，**同时更新**本 README 总表与对应任务文档「进度」章节，二者保持一致。
2. 完成任务需满足该文档的「验收标准」，并在其进度时间线追加一行记录（日期 + 结果 + 验证命令）。
3. 状态只允许在 ⬜ → 🟦 → ✅（或 ⏸️）之间流转，回退需在时间线注明原因。
4. 新增/调整任务时，重新编号并同步依赖字段与本表。
5. **布局基准**：任何面板的结构/尺寸/区域划分以 `../desktop-skin-plan.md` §5 为准；开发中发现 spec 需要调整时，先改 spec 再改代码，并在任务时间线注明。
6. 平台兼容性（窗口状态、主题监听等 Tauri API）遵守根目录 `AGENTS.md`「平台兼容性开发规范」。
