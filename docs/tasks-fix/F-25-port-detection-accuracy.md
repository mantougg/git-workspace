# F-25 端口检测不准确：有端口仍被占用却判定空闲

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 2026-09-02 用户反馈（第 1 项） |
| 关联任务 | R-16（端口管理）、R-14（启动前预检） |

## 问题描述

端口明明还在被占用，但界面或预检结果有时会显示为空闲。这个误判会直接影响
启动前判断和端口诊断结果，用户会以为端口可用，实际启动后才撞错。

## 定位线索

- src-tauri/src/runtime/port_manager.rs 的 check_port 目前以 127.0.0.1 作为 bind
  兜底，口径偏窄
- src-tauri/src/runtime/launch/port_preflight.rs 的 preflight 也沿用同样的 loopback
  判定
- src-tauri/src/process/port.rs 的 detect_port_occupier 只在 bind 失败后补充占用方
  信息
- 前端诊断入口在 src/components/runtime/PortDiagnosticsModal.vue 与
  src/views/PortToolView.vue

## 修复范围

- [ ] 收敛 runtime_check_port 与启动前预检的端口判定口径
- [ ] 覆盖更完整的监听形态与占用场景，避免只测 loopback
- [ ] 保持占用方 PID / 进程名 / 可执行路径的可行动提示
- [ ] 补充端口空闲 / 占用 / 误判回归测试

## 验收标准

- [ ] 端口被占用时不会再误报空闲
- [ ] runtime_check_port 与 preflight 的结论一致
- [ ] 端口诊断仍能给出占用方信息
- [ ] 相关测试通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-02 录入需求，待拆分实现

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ⬜ | 用户反馈录入：端口检测有误判，需统一检测口径并补回归 |
