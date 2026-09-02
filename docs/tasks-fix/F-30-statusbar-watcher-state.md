# F-30 StatusBar 工作区右侧 watcher 点接入真实状态

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-02 用户反馈（第 6 项） |
| 关联任务 | D-03（StatusBar）、Git watcher / 文件监听 |

## 问题描述

StatusBar 里工作区右边那个小点现在 hover 一直显示“未启动”，看起来像一个静态占位。
用户期望它反映当前文件监听的真实状态，而不是永远不变。

## 定位线索

- src/components/shell/StatusBar.vue 里的 watcherActive 目前是本地 ref(false)
- src-tauri/src/core/watcher.rs 已经有实际 watcher 的 started 状态
- src-tauri/src/commands/git_ops.rs 的 start_watcher / stop_watcher 是监听起停入口
- 各 Git 视图当前各自管理 watcher，StatusBar 还没接入统一状态源

## 修复范围

- [x] 为 StatusBar 暴露真实 watcher 状态
- [x] hover 文案与颜色同步真实运行状态
- [x] Git 视图启停 watcher 时即时反映到状态栏
- [x] 刷新或重启后状态不丢失

## 验收标准

- [x] 点位 hover 不再固定显示“未启动”
- [x] watcher 启停后状态及时切换
- [x] 状态与实际监听状态一致
- [x] 相关测试通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 修复完成，查询与启停事件同步已落地

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | ⬜ | 用户反馈录入：StatusBar watcher 状态需要接真实数据 |
| 2026-09-02 | 🟦 | 开始修复：为 StatusBar 接入后端 watcher 状态查询和启停事件 |
| 2026-09-02 | ✅ | 修复完成：新增 watcher 状态查询 IPC，启停后广播状态事件，StatusBar 查询并订阅真实状态；验证 pnpm build、cargo check --all-targets、cargo test --lib core::watcher 通过 |
