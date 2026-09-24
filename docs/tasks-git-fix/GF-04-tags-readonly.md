# GF-04 Tags 只读孤岛（分支管理页能看到标签但完全不能操作）

> 状态：⬜ 未开始
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

分支管理页的 Tags 面板只渲染标签列表（name / message / oid），行尾有一个**空
`<span/>` 占位**（原本应是动作菜单的位置）——用户能看到仓库有哪些标签，但没有任何
删除 / 推送 / 检出入口。根因是后端 `branch.rs` 根本没有 tag 变更类命令，前端无从做起。

## 定位线索（证据）

- 前端只读渲染 + 空占位：`src/views/BranchManager.vue:136-144`（`:141` 空 `<span />`）
- 后端缺口：`src-tauri/src/commands/branch.rs` 共 9 个命令（list/create/checkout/delete/
  rename/set_upstream/track/push/compare），**无任何 tag 命令**
- `list_branches` 已把 tags 快照落库（`src-tauri/src/commands/branch.rs:32-42`），数据侧已有基础。

## 修复范围 checklist

- [ ] 1. 后端 `branch.rs` 新增 tag 命令：`create_tag`（name + 可选 message + 可选目标 oid，缺省 HEAD）、`delete_tag`、`push_tag`（含 force 参数，默认禁用）。
- [ ] 2. 前端 Tags 行加 `n-dropdown` 动作菜单（放在现空占位处）：创建标签（面板级按钮）、推送、删除。
- [ ] 3. 删除按 Roadmap §46 危险分级走二次确认（Dangerous：删除已推送标签需明示）；force push 默认关闭并提示 "This may overwrite remote history."，推荐 `--force-with-lease`。
- [ ] 4. tag 操作后随 `runOp` 统一刷新（引用 `BranchManager.vue:829-837` 的成功/失败模式）。

## 不做（范围控制）

- 不做标签排序/过滤/分组等列表增强。
- 不做「检出 tag」（游离 HEAD 检出属高危低频操作，另立任务评估）。

## 验收标准

1. 能在分支页对当前 HEAD 创建带注释标签，列表即时刷新。
2. 推送标签到远程成功；删除本地标签有确认且本地列表刷新。
3. 删除已推送标签 / force push 的确认流程符合 Roadmap §46。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）与 `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（前端只读孤岛 + 后端无 tag 命令，双缺口）。待复现与修复。 |
