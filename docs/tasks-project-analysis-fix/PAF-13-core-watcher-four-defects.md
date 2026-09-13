# PAF-13 core watcher 四缺陷（debounce 丢弃 / NonRecursive 盲区 / mount 不回滚 / 路径未归一化）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
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

- [ ] 连续保存两个不同文件，两次变更均触发状态刷新
- [ ] 子目录内编辑触发刷新
- [ ] mount 失败的仓库后续可恢复监听
- [ ] Windows 盘符大小写/分隔符混合路径可命中
- [ ] `cargo test --lib` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
