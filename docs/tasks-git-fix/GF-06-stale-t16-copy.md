# GF-06 过期文案误导用户（「三方解决器随 T-16 提供」，T-16 早已交付）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

提交图与分支管理页仍有三处历史文案写着「三方解决器随 T-16 提供」，而 T-16（冲突
解决器 ConflictResolver）早已完整交付。用户看到「随 T-16 提供」会误判「现在没有这个
功能」而绕路去终端手动解决冲突。

## 定位线索（证据）

- `src/views/GitGraph.vue:40`（banner 文案）、`src/views/GitGraph.vue:102`（同类）
- `src/views/BranchManager.vue:63`（merge/rebase 中断恢复 banner）
- T-16 交付物：`src/views/ConflictResolver.vue`（701 行完整实现）与
  `src/components/git/SmartMergeDialog.vue`。

## 修复范围 checklist

- [ ] 1. 删除或改写三处文案；冲突 banner 直接提供「打开解决器」动作（GitGraph 已有进入解决器入口，确认 banner 同样可达）。
- [ ] 2. 全 `src/` grep「T-16」清零（视图文案层面）。
- [ ] 3. 检查其余视图是否有同类「随 T-XX 提供 / 尚未开发」死文案（GF-04 之外的扫描），一并清理。

## 不做（范围控制）

- 不改 ChangeSetView「Create PRs（T-29）尚未开发」disabled 按钮（T-29 已交付但入口为何 disabled 需单独核实，另立任务——复现时确认）。
- 不做 ConflictResolver「AI 建议（T-26）」死按钮（同上，T-26 已完成仍是 disabled，核实后另立任务）。

## 验收标准

1. 三处文案替换后语义正确（功能已存在 → 文案引导去用）。
2. GitGraph 冲突 banner 可一键进入解决器。
3. `grep -rn "T-16" src/` 无业务文案残留。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（三处过期文案，均静态核验）。待修复。 |
