# PAF-01 中文 commit message 触发 UTF-8 字节边界 panic

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P0-1，主控亲自逐行验证 |
| 关联任务 | T-05（任务队列）、TM-04（Git 输出镜像） |

## 问题描述

`src-tauri/src/task/worker.rs:362-363`：

```rust
let short_msg = if message.len() > 50 {
    format!("{}…", &message[..47])
```

`String::len()` 返回字节数，`&message[..47]` 按字节切片。中文 commit
message（UTF-8 每字 3 字节）在第 47 字节落在多字节字符中间时直接 panic
（`byte index is not a char boundary`），Commit 任务被标记为
`Failed{error: "Worker panic: ..."}`，用户提交失败且报错难懂。该路径用于
TM-04 Git Console 合成命令标题，属于提交热路径。

## 定位与修复建议

- 按字符截断而非字节：`message.chars().take(N).collect::<String>()`，
  或用 `char_indices()` 找最近边界。
- 顺带核对展示语义：阈值 50 是字节还是字符，建议统一按字符。
- 加单测：长中文消息触发截断不 panic。

## 验收标准

- [x] 中文 commit message 超过阈值时不再 panic，任务正常完成
- [x] 新增回归测试（中文长消息截断）
- [x] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入（证据：worker.rs:362-363 源码实证） |
| 2026-09-13 | ✅ | 证据复核成立（`execute_task` 内 Commit 分支）。新增 `shorten_message`：按**字符**计数与截断（阈值 50 字符，截断保留 47 字符 + `…`），替换原 `&message[..47]` 字节切片。回归测试 6 项（含 60 汉字 / 字节超限字符未超限 / 混排遍历）。验证：`cargo test --lib` 877 passed |
