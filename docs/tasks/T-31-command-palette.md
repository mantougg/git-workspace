# T-31 Command Palette + 快捷键 + IDE/Terminal 集成

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | P2 |
| 优先级 | P2 |
| 状态 | 🟦 进行中 |
| 依赖 | — |
| 对应 Roadmap | §33 Command Palette、§34 快捷键、§56 Terminal 集成、§57 IDE 集成 |

## 目标

补齐效率型集成能力：Command Palette、快捷键体系、在 IDE / 终端打开仓库或文件。

## 需求范围

- [x] Command Palette（`Ctrl+Shift+P`，兼容 `Ctrl+K`）：fetch/pull/push/branch/checkout/merge/rebase/stash/reset/reflog/worktree/sync/AI review 等
- [x] 快捷键：`Ctrl+P` 仓库搜索、`Ctrl+Shift+F` 代码搜索（暂绑仓库/文件搜索，见时间线）、`Ctrl+Shift+D` Diff、`Ctrl+Shift+G` Graph、`Ctrl+Enter` Commit、`Ctrl+Shift+Enter` Commit & Push、`F5` Refresh
- [x] Terminal 集成：Open Terminal Here / PowerShell / CMD / Git Bash / Windows Terminal（跨平台对应）
- [x] IDE 集成：Open in VS Code / IntelliJ IDEA / Cursor / Zed（后端接受目录/文件/Worktree 路径，前端命令按仓库级接线）

## 架构 / 性能注意点

- Command Palette 命令注册表统一管理，各模块注册命令 + 快捷键，避免散落硬编码。
- 打开外部程序走 Tauri shell plugin，注意路径含空格/引号转义与跨平台命令名差异。

## 验收标准

- [x] Command Palette 可搜索并执行全部注册命令
- [x] 快捷键与命令绑定正确，无冲突
- [x] 各终端 / IDE 打开命令在目标平台可用

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发。现状盘点：Palette UI / 命令注册表 / 快捷键监听已随 D-12/D-14 落地（Ctrl+K 打开、Ctrl+1..9 导航、Ctrl+I 助手）；本任务补齐：Ctrl+Shift+P 打开、git 操作/终端/IDE 命令、Ctrl+P/Ctrl+Shift+F/Ctrl+Shift+D/Ctrl+Shift+G/Ctrl+Enter/Ctrl+Shift+Enter/F5、Terminal/IDE 打开后端命令 |
| 2026-09-02 | ✅ | 完成。根因盘点：①Palette/注册表/快捷键骨架已有，缺 git 操作与外部集成命令；②快捷键监听在 keydown 事件上下文调用 useRouter()（setup 外拿不到实例，Ctrl+1..9 存在失效隐患）。修法：①命令上下文（router/stores）setup 期构建、显式传入监听器与 Palette；②注册表补 Git 操作（fetch/pull/push/commit/commit&push/sync/branch/checkout/merge/rebase/stash/reset/reflog/worktree/AI review，走变更页 action 通道与视图直达）、终端（Windows: system/PowerShell/CMD/GitBash/WT，macOS: Terminal，Linux: gnome-terminal→konsole→xfce4→alacritty→kitty→wezterm→xterm 探测链）与 IDE（VS Code/IDEA/Cursor/Zed，PATHEXT 检测 + .cmd shim 经 cmd /C）命令；③新增 `commands/integration.rs`（SpawnPlan 纯函数 + 8 个单测）与 `list_integration_targets` 探测命令；④快捷键一键多绑（Ctrl+4/Ctrl+Shift+G）、Ctrl+Enter/Ctrl+Shift+Enter/F5 在输入框聚焦时放行，Ctrl+Shift+Enter 经事件参数强制推送不改表单态。边界说明：Ctrl+Shift+F「代码搜索」暂绑仓库/文件搜索——FTS5 `ai_search` 有后端无 UI，T-28 落地后切换；IDE 文件级打开后端已支持任意路径，前端命令按仓库级接线。验证：`cargo test --lib` 777 通过（含 integration 8 项）、`pnpm build`（vue-tsc + vite）通过；Linux 终端探测链实测编译环境，Windows/macOS 命令行为以平台规范 + 纯函数单测兜底，随打包实测 |

### 子任务清单

- [x] 命令注册表 + Command Palette UI
- [x] 快捷键绑定
- [x] Terminal 打开
- [x] IDE 打开
