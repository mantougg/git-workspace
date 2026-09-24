# GF-13 git 小缺陷集合（Reflog 上限 / 分支条截断 / pick_continue 丢作者）

> 状态：✅ 已完成（a/b/c 三子项均修复并验证，2026-09-24）
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

## 定位线索（证据，2026-09-24 复现核验——三子项全部坐实）

- a：`src/views/Reflog.vue` 的 `load()`（原硬编码 `getReflog(repoPath, reference, 200)`，
  修复前行号在 ~167）+ `src-tauri/src/commands/reflog.rs:13`
  （`max.unwrap_or(200)`）。**核验成立**。进一步核验：后端 `max: Option<usize>`
  **早已参数化**（直通 `core/reflog.rs::read_reflog` 的 `take(max)`），故按 checklist
  的退路只做前端翻页状态（「加载更多」+ 条数提示），后端零改动。
  （原文档第一条「BranchManager 远程分支 is_current」属已撤销子项的残留条目，
  随 PAF-26 结论一并删除。）
- b：`src/views/GitGraph.vue` 分支条 `branches.slice(0, 10)`（GF-11 改后行号漂移，
  复现时在 `:19`）。**核验成立**。`docs/desktop-skin-plan.md:264-279`（§5.6）确认
  规范原文即「前 10 个分支」——结论：**按规范补 +N 展开**（点击 popover 列出其余
  分支）而非维持现状，既对齐规范又消除静默截断。
- c：`src-tauri/src/core/history.rs` 的 `pick_continue`（入口，原 ~267）、
  `signature_or_default`（原 ~283）、`repo.commit(Some("HEAD"), &sig, &sig, ...)`
  双当前签名（原 ~291）。**核验成立**，project-analysis:193 记录属实。
  既有测试 `cherry_pick_conflict_then_continue_commits`（`core/history.rs` tests
  模块，原 ~521 起）已按验收要求扩展原作者/committer 断言。
  补充核验：libgit2 1.8.1 `cherrypick.c::write_cherrypick_head` 确认冲突时会把
  **被 pick 提交的 oid 字符串**写入 `.git/CHERRY_PICK_HEAD`（该文件本就是
  pick_continue 判定 in_pick 的依据），取回原提交 author 的路径可靠。

## 修复范围 checklist

- [x] a. Reflog：显示「已显示前 N 条」+「加载更多」按钮。后端 `max` 本已参数化
      （`commands/reflog.rs` `Option<usize>`），前端补翻页状态：`Reflog.vue`
      `PAGE_SIZE=200` / `requestedMax` / `hasMore` / `allLoaded` + `loadMore()`，
      页脚显示「已显示前 N 条 / 已显示全部 N 条」（短页探测到底，零后端改动）。
- [x] b. GitGraph 分支条：超出 10 个时显示「+N」并点击展开（n-popover + n-scrollbar
      列出其余分支，tag 配色复用 `branchTagType()`）；与 desktop-skin-plan §5.6
      「前 10 分支」规范对齐。GF-11 的翻页 / remount key 状态管理未受影响。
- [x] c. `pick_continue` 保留原 commit author：新增
      `core/history.rs::cherry_pick_head_author()`——从 `.git/CHERRY_PICK_HEAD`
      （libgit2 冲突时写入的被 pick 提交 oid）取原提交 `author().to_owned()` 作
      author，committer 保持 `signature_or_default` 当前用户；revert 路径无原作者
      概念维持现状。**回退语义**：CHERRY_PICK_HEAD 缺失 / 内容非法 / 提交不存在时
      `log::warn!` 记录并回退当前签名（修复前行为，不阻断冲突恢复）。
      测试 `cherry_pick_conflict_then_continue_commits` 已扩展断言（`git log
      --format='%an %cn'` 等价）：author = "original Author"、committer = "current-user"。

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
| 2026-09-24 | 修复完成（a/b/c 三子项）：**a 根因**——`Reflog.vue` 硬编码传 200、后端默认亦 200，超限静默截断无提示；**修法**——后端 `max` 早已参数化故零改动，前端 `Reflog.vue` 加分页状态（`PAGE_SIZE=200`/`requestedMax`/`hasMore`/`allLoaded` + `loadMore()`）与页脚条数提示（「已显示前 N 条 / 已显示全部 N 条」，短页探测到底）。**b 根因**——`GitGraph.vue` `branches.slice(0, 10)` 静默丢弃其余分支；**修法**——超出 10 个显示 `+N` 标签，n-popover（click 触发）+ n-scrollbar 展开列出其余分支，tag 配色抽为 `branchTagType()`，与 desktop-skin-plan §5.6「前 10 分支」规范对齐；GF-11 翻页/refresh key 未动。**c 根因**——`core/history.rs::pick_continue` 用 `&sig, &sig` 让当前用户同时充当 author/committer，cherry-pick 原作者丢失（干净路径 `cherry_pick` 却保留了 `commit.author()`，仅冲突恢复路径漏掉）；**修法**——新增 `cherry_pick_head_author()` 从 `.git/CHERRY_PICK_HEAD`（libgit2 冲突时写入被 pick 提交 oid）取原提交 author 作 author、committer 保持当前用户；回退语义：文件缺失/oid 非法/提交不存在时 warn 并回退当前签名。**验证**——`cargo test --lib core::history` 6/6 通过（含扩展断言 author="original Author"、committer="current-user"）；`git archive HEAD src-tauri` 抽树 + 叠加本改动的全量 `GW_TEST_MANIFEST=1 cargo test --lib`：1009 passed / 15 failed，失败集与 HEAD 基线（15 failed，含 real_maven×9、node workspace×2、pathutil 大小写、pty smoke、real_node_vite flaky）一致，无新增失败；`pnpm build`（vue-tsc --noEmit && vite build）通过。impact：`pick_continue`(core) 上游仅测试 1 处（LOW）、`read_reflog` 上游 4 处均为命令与测试（LOW），无 HIGH/CRITICAL。 |
