# GF-13 git 小缺陷集合（Reflog 上限 / 分支条截断 / pick_continue 丢作者）

> 状态：⬜ 未开始
> 优先级：P2（三个独立小修，参照 F-09 集合模式合并跟踪）
> 来源：2026-09-24 Git 使用体验全景盘点（其中一项来自 2026-09-13 项目分析报告未修清单）。
> 核验记录（2026-09-24）：初版含 4 个子项，子项「远程分支 is_current 误判（contains）」
> 经源码核验**已由 PAF-26 修复**（`src-tauri/src/core/graph.rs:325-336`：按
> `{remote}/{branch}` 组件级精确比较，代码注释明确记载 PAF-26），project-analysis:192
> 的记录已过时——撤销该子项。

## 问题描述（3 个子项）

### a. Reflog 200 条硬编码上限无提示

前端固定传 200（`Reflog.vue:167`），后端默认亦 200（`src-tauri/src/commands/reflog.rs:13`，
`max.unwrap_or(200)`）——超过 200 条**静默截断**，无「加载更多」、无总数提示，用户
不知道历史被藏了。

### b. GitGraph 分支条只显示前 10 个

`src/views/GitGraph.vue:19` `branches.slice(0, 10)`——其余分支无提示、无展开方式。多分支仓库
用户不知道还有分支（desktop-skin-plan.md:264-279 的提交图规范写的就是「前 10 分支」，
需与规范对齐后决定是补展开还是接受现状并加提示）。

### c. pick_continue 丢失原 commit 作者

cherry-pick 冲突解决后 continue 时，新提交用当前用户签名**同时充当 author 和
committer**（`src-tauri/src/core/history.rs:283` 取 `signature_or_default`，`:291`
`repo.commit(Some("HEAD"), &sig, &sig, ...)` 两个参数都是当前签名）——原作者丢失，
违反 cherry-pick 语义（应保留原 commit 的 author，committer 才是当前用户）。原
commit 可从 `CHERRY_PICK_HEAD` 文件中的 oid 取回。project-analysis:193 记录属实。

## 定位线索（证据）

- a：实现位置在 `src/views/BranchManager.vue` 远程分支渲染（`is_current` 计算，具体行号复现时定位）；分析报告 [../project-analysis-2026-09-13.md:192](../project-analysis-2026-09-13.md)
- b：`src/views/Reflog.vue:167` + `src-tauri/src/commands/reflog.rs:13`
## 定位线索（证据）

- a：`src/views/Reflog.vue:167` + `src-tauri/src/commands/reflog.rs:13`
- b：`src/views/GitGraph.vue:19`
- c：`src-tauri/src/core/history.rs:267`（pick_continue 入口）、`:283`（signature_or_default）、
  `:291`（`&sig, &sig` 双当前签名）；命令层 `src-tauri/src/commands/history.rs:83`；
  既有测试 `src-tauri/src/core/history.rs:521` 起（cherry-pick conflict → pick_continue）

## 修复范围 checklist

- [ ] a. Reflog：显示「已显示前 200 条」+「加载更多」按钮（后端 limit 参数化）。
- [ ] b. GitGraph 分支条：超出 10 个时显示「+N」并可展开/悬停列出其余。
- [ ] c. `pick_continue` 保留原 commit author（从 `CHERRY_PICK_HEAD` 的 oid 取原提交
  signature 作 author；committer 保持当前用户；revert 路径同理无原作者概念，维持现状）。

## 验收标准

1. Reflog 超 200 条有「加载更多」与条数提示。
2. >10 分支仓库：分支条可查看全部分支。
3. cherry-pick 冲突解决后 continue，`git log --format='%an %cn'` 显示原 author + 当前 committer。
4. 每子项独立勾选；`cargo test --lib`（GW_TEST_MANIFEST=1）与 `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：四子项合并跟踪（源含 project-analysis 未修清单）。 |
| 2026-09-24 | 源码核验：子项「is_current contains 误判」已被 PAF-26 修复（`core/graph.rs:325-336`），撤销；其余三子项全部坐实（pick_continue 的 `&sig, &sig` 在 `core/history.rs:291`）。改为三子项 a/b/c。 |
