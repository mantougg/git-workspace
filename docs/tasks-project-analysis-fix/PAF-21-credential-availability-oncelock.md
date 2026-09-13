# PAF-21 凭证可用性 OnceLock 缓存 + get 静默降级

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-24，主控亲自验证 |
| 关联任务 | AI-01（凭证管理） |

## 问题描述

`src-tauri/src/ai/credentials.rs`：

1. :49 `available: OnceLock<bool>` 可用性探测进程生命周期只算一次；:48
   注释宣称「`refresh_availability` 可重测」但**方法从未实现**——进程启动
   时若 gnome-keyring/Secret Service 尚未解锁，可用性永远为 false，用户
   只能用「仅本次会话」凭证直到重启应用。
2. :241-246 `CredentialManager::get` 在 OS 后端返回 Err（后端不可用）时
   **静默吞掉降级查会话**——后端短暂故障 + 会话有旧副本时可能读到过期
   Key（单落点不变式只在写入时维护）。

## 定位与修复建议

- 实现 `refresh_availability`（或在 get/set 失败时惰性重测）；
- `get()` 区分「后端不可用」与「无条目」：前者告警或显式降级提示，而非
  静默读会话副本。

## 验收标准

- [ ] keyring 晚解锁场景下无需重启即可恢复使用 OS 凭证
- [ ] OS 后端故障时的降级对用户可见（日志/UI 提示）
- [ ] `cargo test --lib`（ai::credentials）通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
