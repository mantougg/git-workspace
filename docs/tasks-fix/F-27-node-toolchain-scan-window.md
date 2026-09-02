# F-27 Node 工具链扫描闪窗且包管理器探测失败

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-02 用户反馈（第 3 项） |
| 关联任务 | N-01 / N-10（Node 工具链扫描）、平台兼容性规范 |

## 问题描述

在本机扫描 Node 工具链时，Windows 上会弹出多个一闪而过的终端窗口；同时包管理器
经常全部探测失败，只有 node 能成功。

## 定位线索

- src-tauri/src/node/scan.rs 的 scan_node_toolchain 会对每个候选执行版本探测
- src-tauri/src/node/detect.rs 的 build_probe_command 目前没有复用统一的无窗口 spawn
  逻辑
- src-tauri/src/node/scan.rs 的 scan_roots 只覆盖固定安装目录与部分管理器目录
- Windows 平台已有无控制台 spawn 习惯可参考 src-tauri/src/process/streaming.rs

## 修复范围

- [ ] 扫描候选覆盖补齐常见全局安装路径与 PATH 发现面
- [ ] 版本探测统一改成无可见控制台窗口的执行方式
- [ ] 明确包管理器探测失败的可行动错误与注册策略
- [ ] 补 Windows 扫描 / 探测回归测试

## 验收标准

- [ ] Windows 扫描不再闪终端窗口
- [ ] 已安装的 npm / pnpm / yarn / bun 可被稳定发现
- [ ] node 与包管理器都能按各自路径探测版本
- [ ] 相关测试通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 修复完成，扫描与探测已通过 Windows 实测

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ⬜ | 用户反馈录入：扫描闪窗与包管理器探测失败需要一起处理 |
| 2026-09-02 | 🟦 | 开始修复：确认 PATH 扫描缺口与 Windows 探测进程未隐藏 |
| 2026-09-02 | ✅ | 修复完成：探测命令增加 CREATE_NO_WINDOW；扫描加入 PATH 目录并清理 Windows verbatim 路径；Windows 实测发现并成功探测 npm、pnpm、yarn、bun。验证：node::detect 10 passed、node::scan 9 passed、rustfmt --check 通过。全量 node 测试另有 2 个既有 workspace verbatim 路径失败和 1 个真实 Vite 5176 连接环境失败，未纳入本任务范围 |
