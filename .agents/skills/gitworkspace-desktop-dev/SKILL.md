---
name: gitworkspace-desktop-dev
description: GitWorkspace 桌面化改造任务流程：如何读 docs/tasks-desktop/ 文档（总索引/任务spec）开始与继续 D-XX 任务（Desktop Skin 改造）开发、并同步进度。
---

# GitWorkspace 桌面化改造任务流程

本 skill 教你在 **GitWorkspace** 项目中，如何基于 `docs/tasks-desktop/` 的任务文档**开始开发**或**继续开发**某个桌面化改造任务（D-XX 编号）。

## 文档地图

桌面化改造任务已拆解在 `docs/tasks-desktop/` 下：

| 文件 | 作用 | 何时读 |
|---|---|---|
| `docs/tasks-desktop/README.md` | 总索引：16 个 D-XX 任务的阶段/优先级/状态/依赖总表 + 依赖链 + 维护规范 | 选任务、核对状态时 |
| `docs/tasks-desktop/D-XX-*.md` | 任务 spec：目标 / 需求范围 checklist / 验收标准 / 进度 | 开发目标任务时 |
| `docs/desktop-skin-plan.md` | **设计 spec（布局基准）**：三层 Desktop Skin、§5 面板结构布局详述、分期与验收 | 开发任何 D-XX 任务前**必读对应章节** |

约束文档（按需加载，不重复读）：

- 平台兼容性（窗口状态 / 主题监听 / Tauri API）→ 根目录 `AGENTS.md` 的「平台兼容性开发规范」
- 状态栏版本槽位数据源 → 根目录 `AGENTS.md` 的 F-07 条目（`__APP_VERSION__` / `__APP_AUTHOR__`）

## 开始开发一个新任务

1. 确定任务编号（用户指定，或从 README 总表选一个「依赖均已 ✅」的任务）。
2. 读 `README.md` 总表，确认该任务的状态、优先级、直接依赖。
3. 读目标任务文档，明确：目标、需求范围（checklist）、验收标准。
4. 读 `docs/desktop-skin-plan.md` 中该任务「对应方案」标注的章节——**布局/视觉以 spec §5 为准，不要凭记忆自由发挥**。
5. 把任务状态 `⬜ → 🟦`（**同步**更新 README 总表 + 任务文档「进度」章节），并在时间线追加一行「开始开发」。
6. 开始实现。

## 继续开发（恢复一个进行中的任务）

1. 读目标任务文档「**进度**」章节：当前状态 + 时间线最后一条 + 子任务清单勾选情况。
2. 读 `README.md` 总表该任务行，核对两处状态一致（不一致时以任务文档为准，并修正 README）。
3. 从时间线最后一条记录恢复上下文，继续**未勾选的子任务**。

## 完成一个任务

1. 逐条核对「验收标准」，**全部满足**才算完成；有布局类验收的对照 `desktop-skin-plan.md` §5 面板图目检。
2. 运行相关验证（`pnpm build`（含 vue-tsc）、`cargo check`，按改动范围选择）。
3. 更新任务文档「进度」：状态 `→ ✅`，时间线追加一行（日期 + 结果 + 验证命令）。
4. 同步更新 README 总表该任务状态 `→ ✅`，并重算「总体进度」计数。
5. 若存在依赖此任务的下游任务，提示用户可开始下游。

## 必须遵守

- **spec 优先**：布局结构、尺寸、区域划分以 `docs/desktop-skin-plan.md`（尤其 §5）为准；发现 spec 需要调整时，先改 spec 再改代码，并在任务时间线注明。
- **进度两处同步**：状态流转与维护规则以 `docs/tasks-desktop/README.md` 末尾「维护规范」为准。
- **最小改动**：一期任务（D-01~D-06）不动任何视图的业务逻辑与 IPC 调用，纯壳层 + 导航清理。
- **代码落点**：骨架组件在 `src/components/shell/`，tokens 在 `src/styles/tokens.scss`，命令注册表在 `src/commands/`（2.5 期起），Vue 视图在 `src/views/`。
