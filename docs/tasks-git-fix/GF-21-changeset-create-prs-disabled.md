# GF-21 ChangeSetView「Create PRs」disabled 死按钮（T-29 已交付）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：GF-06 复现时牵出（2026-09-24）。

## 问题描述

`src/views/ChangeSetView.vue:160` 的「Create PRs」按钮 `disabled` 且 tooltip 写
「由 T-29 实现（尚未开发）」——但 T-29 已完整交付：前端 `src/api/remote.ts:34`
`createPullRequest`（invoke `create_pull_request`）+ 后端
`src-tauri/src/commands/remote.rs:171`（平台 REST，异步）均在，且 BranchManager
PR 面板已有 token 输入 UI。用户看到 disabled 按钮会误判功能不存在。

## 定位线索（证据）

- disabled 按钮 + 死文案：`src/views/ChangeSetView.vue:160`（GF-06 复现时核对）
- API wrapper：`src/api/remote.ts:34`（`createPullRequest(input)` → `create_pull_request`）
- 后端命令：`src-tauri/src/commands/remote.rs:171`（async，平台 REST）
- token 入口：BranchManager PR 面板 token 输入（remote.rs `save_remote_token`）

## 待核验（复现优先）

1. ChangeSetView 的数据形态能否映射到 `create_pull_request` 的入参
   （repository / head / base / title / body / token 来源）？
2. 按钮 disabled 是否有其他前置条件（如未检测 remote / 无 platform token）被
   文案掩盖？若是，接上 GF-08 的 token 缺失引导（`RemoteAuthRequired`）。
3. 与 BranchManager 现有 PR 面板的关系：合并入口还是差异化定位？

## 修复范围 checklist

- [ ] 1. 核验入参映射与 disabled 真实原因。
- [ ] 2. 恢复按钮可用（或改文案明示真实前置条件），走现有 api + 错误引导。
- [ ] 3. 清理「尚未开发」死文案。

## 验收标准

1. 具备 PR 创建条件的仓库：按钮可点，创建流程走通（构造/mock 验证）。
2. 缺 token/远程不可达：错误提示含可行动引导（GF-08 RemoteAuthRequired）。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：GF-06 复现确认 T-29 已交付（api/remote.ts + commands/remote.rs 均在），按钮仍 disabled 且文案死。待核验与修复。 |
