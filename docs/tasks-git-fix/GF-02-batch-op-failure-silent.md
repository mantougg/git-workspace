# GF-02 批量操作失败零反馈（批量 Pull 静默吞失败、watcher 启停失败不提示）

> 状态：⬜ 未开始
> 优先级：P0（用户看到「3 个仓库 Pull 完成」而不知道有 2 个失败了——信任受损型缺陷）
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

两处「操作失败但用户看不到」的模式：

1. **批量 Pull**：`handlePull` 内层 `catch {}` 注释写着「Individual repo failure doesn't stop
   the batch」，但失败仓库既不进 `successCount` 也不进冲突队列——最终只 toast
   「N 个仓库 Pull 完成」，失败的仓库凭空消失。
2. **文件监听启停**：`startFileWatcher` 失败只 `console.error`；
   `toggleWatcher` 的三元失败分支不弹任何提示——用户点了「启动监听」没有任何反馈。

## 定位线索（证据）

- 批量 Pull 静默 catch：`src/views/RepositoryList.vue:2455-2457`
- watcher 启动失败仅 console.error：`src/views/RepositoryList.vue:2560-2562`
- toggleWatcher 失败分支无 toast：`src/views/RepositoryList.vue:2575-2578`
- 批量操作入队路径（任务面板已有）：`git_ops.rs` 的 `batch_pull/batch_push/batch_fetch`
  （`src-tauri/src/commands/git_ops.rs:43/65/87`），失败时 task 状态带 error 字符串——
  **后端有数据，前端没有汇总展示**。

## 修复范围 checklist

- [ ] 1. 批量 Pull/Push/Fetch 收口时汇总失败清单（仓库名 + 简短原因），toast 明示，并提供「查看任务面板」动作。
- [ ] 2. watcher 启动/停止失败：`message.error` 展示错误原因。
- [ ] 3. 检查其余批量入口（批量提交、批量 branch op、批量 add/restore）是否有同样模式，有则一并收口。

## 不做（范围控制）

- 不改变「单仓失败不中断批次」的语义（这是合理设计，缺的是反馈）。
- 不重构任务队列错误上报链路（后端 error 字符串已有）。

## 验收标准

1. 构造一个远程不可达的仓库混入批量 Pull：完成后 toast 明确列出失败仓库与原因，成功数不含它。
2. watcher 启动失败（如制造一个不可监听路径）时有错误提示，不再静默。
3. 全成功场景提示与当前一致，无回归。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（静态证据见上）。待复现与修复。 |
