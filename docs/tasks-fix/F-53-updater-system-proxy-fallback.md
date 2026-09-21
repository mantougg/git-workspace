# F-53 检查更新报「error sending request for url」（GitHub 直连失败，未走系统代理）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-20 用户反馈：关于页点「检查更新」报 `error sending request for url (https://github.com/mantougg/git-workspace/releases/latest/download/latest.json)` |
| 关联任务 | 无 |

## 问题描述

关于页「检查更新」点击后报错 `error sending request for url(...)`。这是
reqwest 的**传输层错误**（连接被重置/不可达），不是 404——GitHub Releases
在当前网络下直连失败。应用内 `check()`（`composables/useUpdater.ts`）不带
任何代理参数，Tauri 打包后的应用从资源管理器启动时也没有 `HTTPS_PROXY`
环境变量，更新检查因此完全裸连 GitHub。

## 根因（已定位）

- updater 端点配置本身正确（`tauri.conf.json` → `releases/latest/download/latest.json`）；
  失败发生在 TCP/TLS 连接阶段。
- `tauri-plugin-updater` 的 `check()` 支持 `proxy` 参数（检查与下载共用该
  客户端配置），但应用未传；reqwest 默认只读代理环境变量，不读 Windows
  IE 系统代理（注册表）。

## 修复范围

- [x] 新增命令 `get_system_proxy`（`commands/app.rs`）：环境变量
      （`HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` 大小写）→ Windows 注册表
      IE 代理（`HKCU\...\Internet Settings`，`ProxyEnable`+`ProxyServer`）；
      纯函数 `normalize_proxy_value` 处理裸 `host:port` / 分协议形式
      `http=…;https=…` / 已带 scheme 三种形态
- [x] `useUpdater.checkForUpdates`：直连 `check()` 抛出传输层错误
      （`error sending request|connect|timeout|dns|tls`）时，取系统代理
      以 `check({proxy})` 重试一次；非传输错误（404/签名校验等）不重试
- [x] 错误提示语义化：代理重试仍失败 →「无法连接 GitHub 更新服务器
      （直连与系统代理均失败）：…」；无代理可用 →「（未检测到系统代理）」
- [x] 单测：`normalize_proxy_value_cases` 覆盖四种形态 + 空值

不在本次范围：macOS `scutil --proxy` 解析（macOS/Linux 回落环境变量，
与 reqwest 默认行为一致）；手动指定代理的 UI 设置项（如后续有需要再立
项）。

## 验收标准

- [x] `GW_TEST_MANIFEST=1 cargo test --lib -- commands::app` 全绿
- [x] `vue-tsc` 通过
- [ ] 真机：断开 shell 代理环境变量、开启 Clash 系统代理时点「检查更新」
      不再报传输错误（待用户实测）

## 进度

### 状态

- 当前状态：✅ 已完成（代码 + 单测 + 类型检查全绿；真机代理重试待用户实测）
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位为传输层失败：updater 裸连 GitHub，未走系统代理 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：`get_system_proxy` 命令（环境变量 + Windows IE 注册表，纯函数规范化）+ `checkForUpdates` 传输错误时代理重试 + 错误提示语义化；`commands::app` 单测全绿，vue-tsc 通过 |
