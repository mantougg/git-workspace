# PAF-13 core watcher 四缺陷（debounce 丢弃 / NonRecursive 盲区 / mount 不回滚 / 路径未归一化）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-14，核查智能体逐项验证 |
| 关联任务 | T-06（File Watcher）、T-02（Status Engine）、F-30 |

## 问题描述

`src-tauri/src/core/watcher.rs` 四个已实证缺陷：

1. **debounce 丢弃尾事件**（:210-213）：窗口内事件直接 `continue` 丢弃而非
   合并延迟——「保存 A → 立即保存 B」时 B 的变更可能永不触发重算，UI 状态
   滞留；与 T-06 文档「同仓库短窗口合并」语义不符。
2. **NonRecursive 监听盲区**（:146-159）：仓库根与 `.git` 均 NonRecursive，
   子目录内文件编辑不产生事件；`.git` 嵌套路径（refs/heads/*）同样盲区，
   commit/push 后刷新依赖 `.git/index`、`HEAD` 恰好是直接子项这一隐式耦合。
3. **mount 失败不回滚**（:64-79）：先把条目写入 `watched` 再 mount，失败
   仅 log::warn——该目录永不重试，状态刷新静默失效且无用户可见错误。
4. **事件路径匹配未归一化**：`core/git_status.rs:320-338 path_under_root`
   只做边界字节 `'/'/'\\'` 双兼容，无整串分隔符归一化、无大小写归一、无
   `\\?\` verbatim 清洗——违反 AGENTS.md §1 自定规则，手动添加的仓库若
   路径写法不一致则事件永远匹配不到。

另：`last_refresh`（:179）卸载后不清理只增不减；`:65/:81/:190` 三处
`lock().unwrap()` 有 Mutex 中毒级联风险（code-review-2026-08-26 亦点名）。

## 定位与修复建议

- debounce 改 trailing-edge 合并（延迟执行而非丢弃）；
- 评估递归监听/按目录事件归属的成本，至少覆盖子目录编辑；
- mount 失败从 watched 剔除并保留重试；
- 事件匹配复用 `maven/index/path.rs::path_key` 归一化后再比较。

## 验收标准

- [x] 连续保存两个不同文件，两次变更均触发状态刷新（窗口内事件合并为 trailing 刷新，最终态不丢）
- [x] 子目录内编辑触发刷新（仓库根改 Recursive；失败回退 root+递归 `.git`）
- [x] mount 失败的仓库后续可恢复监听（失败回滚 `watched`，下次 `watch_repositories` diff 自动重试）
- [x] Windows 盘符大小写/分隔符混合路径可命中（`normalize_path_for_compare` 双侧归一化）
- [x] `cargo test --lib` 通过（896 passed / 0 failed）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据全部成立（锚点未漂移）。修复：① debounce 改 leading+trailing 合并——静默仓库首个事件立即刷新，窗口内事件 `or_insert_with_key` 安排 trailing 刷新（`next_due`/`poll_due` 纯函数 + 单测），最终态不再丢弃；② `mount_repo` 优先 Recursive 覆盖子目录与嵌套 refs，递归失败（如 Linux inotify watch 耗尽）回退 root NonRecursive + 递归 `.git`，root 完全失败才报错；③ mount 前先入 `watched` 集（消除 mount syscall 期间丢事件竞态）、失败即回滚，下次扫描 `sync_watcher` diff 自动重试；④ `find_affected_repos` 双侧经 `normalize_path_for_compare`（仿写 `maven/index/path.rs`：去 `\\?\` verbatim 前缀 + 分隔符归一 + Windows/macOS 大小写折叠，Linux 保留大小写，cfg 门控）。附带：`last_refresh`/`due` 每 flush tick 对 `watched` retain 清理（不再只增不减），3 处 `lock().unwrap()` 改中毒恢复 `lock_watched`。边界说明：递归监听在 Linux 大仓库下若 watch 配额耗尽自动降级为旧行为（root+`.git`）；trailing 合并使静默仓库刷新仍即时、突发仓库最多滞后一个 500ms 窗口。验证：`cargo test --lib`（新增 6 个单测：`next_due`/`poll_due`/归一化匹配×4）。 |
