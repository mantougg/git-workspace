# GF-06 过期文案误导用户（「三方解决器随 T-16 提供」，T-16 早已交付）

> 状态：✅ 已完成
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

- [x] 1. 删除或改写三处文案；冲突 banner 直接提供「打开解决器」动作（GitGraph 已有进入解决器入口，确认 banner 同样可达）。
- [x] 2. 全 `src/` grep「T-16」清零（视图文案层面）。
- [x] 3. 检查其余视图是否有同类「随 T-XX 提供 / 尚未开发」死文案（GF-04 之外的扫描），一并清理。

## 实现说明（2026-09-24）

- **复现结论**：三处文案全部成立（GitGraph.vue 冲突 banner 小字、GitGraph.vue 冲突对话框 note、BranchManager.vue merge banner hint）。
- 修复：
  1. `GitGraph.vue:67` banner hint → 「在解决器中编辑解决，或手动修改文件后提交」；同文件 `:56` 注释去掉 T-16 表述。
  2. `GitGraph.vue` 冲突对话框：note 改写为「可打开解决器编辑解决，或关闭后手动修改文件，也可立即中止」；**footer 新增 primary「打开解决器」按钮**（`openResolverFromDialog`：先关对话框再跳 conflict-resolver）——对话框此前只给「稍后手动解决 / Abort」，解决器入口缺失。
  3. `BranchManager.vue:63` merge banner hint → 「请在解决器中解决冲突并暂存后，点『已解决，继续』」（banner 本身已有「打开解决器」按钮，核验可达）。
- 同类扫描（checklist 3）结论：
  - `DashboardView.vue:184`「批量 Stash 将随 T-21 提供」disabled 按钮——**T-21（Workspace Stash）已交付**（`src/api/workspaceStash.ts` + 变更页面板 + GF-10 刚入任务队列）。清理：按钮改为可用，`quickAction('ws-stash')` 跳变更页 `?action=ws-stash`；`RepositoryList.vue` 的 `applyRoutePrefill` 新增 `case "ws-stash": openWsStashDialog()` 直接唤起面板。
  - `ChangeSetView.vue:160`「Create PRs（T-29）尚未开发」disabled——T-29 已交付（`create_pull_request` 前后端均在），按本任务「不做」边界**另立 GF-21**。
  - `ConflictResolver.vue:18`「AI 冲突建议将在 T-26 提供」disabled——T-26 已交付（`AiConflictAssistant` 正在同文件渲染中），按边界**另立 GF-22**。
  - `registry.ts:98` 的 T-28 注释为代码注释非视图文案，保留。
- 验收 2 grep：视图文案层 `T-16` 仅剩本次修改的说明性注释（记录原文），业务文案零残留。

## 验收标准

1. 三处文案替换后语义正确（功能已存在 → 文案引导去用）。
2. GitGraph 冲突 banner 可一键进入解决器（原有按钮核验在位）+ 冲突对话框新增「打开解决器」。
3. `grep -rn "T-16" src/` 无业务文案残留。

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
| 2026-09-24 | 复现成立（三处文案均在）。修复：GitGraph banner/对话框 note 改写 + 对话框 footer 新增「打开解决器」按钮；BranchManager merge banner hint 改写；同类扫描另清理 DashboardView T-21 死按钮（改可用 + action=ws-stash 预填唤起 Workspace Stash 面板，RepositoryList 接预填 case）；T-29/T-26 两处按边界另立 GF-21/GF-22。验证：`pnpm build`（vue-tsc+vite）通过；视图文案层 T-16 零残留。状态 → ✅。 |
