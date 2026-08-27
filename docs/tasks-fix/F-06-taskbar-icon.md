# F-06 打包启动后任务栏不显示应用图标

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 6 |

## 问题描述

应用打包（`tauri build` 产物）安装/启动后，Windows 任务栏没有显示应用图标（显示为默认空白图标或缺失）。开发态是否正常需一并确认。

## 定位线索

- 图标资源：`src-tauri/icons/`
- Tauri 配置：`src-tauri/tauri.conf.json`（`bundle.icon`、`app.windows[].icon` 等）
- Windows 任务栏图标还与 exe 内嵌资源 / AppUserModelID 有关

## 修复范围

- [x] 确认 `tauri.conf.json` 的 bundle 图标配置完整（ico / png 各平台格式）——已完整，`icon.ico` 验证为合法文件（6 个尺寸）
- [x] 确认窗口创建时指定了图标——**根因在此**：窗口大图标从未被设置，见时间线
- [x] Windows 打包产物实测：任务栏、开始菜单、安装器图标均正常——release exe 实测窗口大/小图标均已设置（WM_GETICON 非 0）；exe 内嵌图标资源经 PowerShell 提取确认是应用图标（NSIS 安装器/快捷方式图标取自同一 exe 资源）
- [x] macOS / Linux 打包图标顺带核对（无环境则在时间线说明未验证平台）

## 验收标准

- [x] Windows 打包产物启动后任务栏正确显示应用图标
- [x] 安装器与快捷方式图标正常

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成。排查：bundle 配置与 `icons/icon.ico`（6 尺寸合法）无问题；exe 内嵌图标资源正常（PowerShell `Icon.ExtractAssociatedIcon` 提取出 GW 图标）。**根因**：实测运行中窗口 `WM_GETICON` 返回 ICON_SMALL 有值、**ICON_BIG = 0**——tao 的 `set_window_icon`（Tauri `set_icon` 同路径）只设置小图标，大图标接口 `set_taskbar_icon` 未被 tauri-runtime-wry 暴露，而 Windows 任务栏按钮用大图标。**修法**：`lib.rs` 新增 `#[cfg(windows)] set_windows_taskbar_icon`——`LoadImageW` 加载 exe 内嵌图标资源（tauri-build 固定以 ID 32512 嵌入 `icon.ico`），`WM_SETICON` 同时设置 ICON_BIG/ICON_SMALL；`Cargo.toml` 新增 Windows-only `windows-sys 0.59`（tauri 传递依赖同源版本）。**验证**：`cargo build --release` 后实跑 exe，`WM_GETICON` 返回 ICON_SMALL=ICON_BIG=同一非零句柄。未验证平台：macOS/Linux（bundle targets 仅 nsis，macOS 已有 icon.icns 配置）。遗留说明：Windows 任务栏有图标缓存，若仍见旧图标可重启资源管理器或重新固定 |

### 子任务清单

- [x] 图标配置核查与修复（配置无误，补窗口大图标设置）
- [x] Windows 打包实测（release exe 实跑 WM_GETICON 验证）
