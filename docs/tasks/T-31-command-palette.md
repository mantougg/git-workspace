# T-31 Command Palette + 快捷键 + IDE/Terminal 集成

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)。

| 项 | 值 |
|---|---|
| 阶段 | P2 |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | — |
| 对应 Roadmap | §33 Command Palette、§34 快捷键、§56 Terminal 集成、§57 IDE 集成 |

## 目标

补齐效率型集成能力：Command Palette、快捷键体系、在 IDE / 终端打开仓库或文件。

## 需求范围

- [ ] Command Palette（`Ctrl+Shift+P`）：fetch/pull/push/branch/checkout/merge/rebase/stash/reset/reflog/worktree/sync/AI review 等
- [ ] 快捷键：`Ctrl+P` 仓库搜索、`Ctrl+Shift+F` 代码搜索、`Ctrl+Shift+D` Diff、`Ctrl+Shift+G` Graph、`Ctrl+Enter` Commit、`Ctrl+Shift+Enter` Commit & Push、`F5` Refresh
- [ ] Terminal 集成：Open Terminal Here / PowerShell / CMD / Git Bash / Windows Terminal（跨平台对应）
- [ ] IDE 集成：Open in VS Code / IntelliJ IDEA / Cursor / Zed（Open Repository / File / Worktree）

## 架构 / 性能注意点

- Command Palette 命令注册表统一管理，各模块注册命令 + 快捷键，避免散落硬编码。
- 打开外部程序走 Tauri shell plugin，注意路径含空格/引号转义与跨平台命令名差异。

## 验收标准

- [ ] Command Palette 可搜索并执行全部注册命令
- [ ] 快捷键与命令绑定正确，无冲突
- [ ] 各终端 / IDE 打开命令在目标平台可用

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 命令注册表 + Command Palette UI
- [ ] 快捷键绑定
- [ ] Terminal 打开
- [ ] IDE 打开
