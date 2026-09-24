# GF-04 Tags 只读孤岛（分支管理页能看到标签但完全不能操作）

> 状态：✅ 已完成
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

分支管理页的 Tags 面板只渲染标签列表（name / message / oid），行尾有一个**空
`<span/>` 占位**（原本应是动作菜单的位置）——用户能看到仓库有哪些标签，但没有任何
删除 / 推送 / 检出入口。根因是后端 `branch.rs` 根本没有 tag 变更类命令，前端无从做起。

## 定位线索（证据）

2026-09-24 复现核验（对照 `git show HEAD:`，HEAD = f470b8c GF-07 定稿后）：

- 前端只读渲染 + 空占位：`src/views/BranchManager.vue:136-144`（`:141` 空 `<span />`）
  —— **成立，无漂移**。HEAD 版本该 Panel 的第 7 行正是 `<span />`（= 文件 141 行），
    行尾无任何动作入口；Tags 面板也没有面板级按钮。
- 后端缺口：`src-tauri/src/commands/branch.rs` 共 9 个命令（list/create/checkout/delete/
  rename/set_upstream/track/push/compare），**无任何 tag 命令**
  —— **成立，无漂移**。HEAD 的 `branch.rs` 逐个 `pub fn` 清点确为 9 个且无 tag 命令；
    `core/branch.rs` 侧同样只有 `tag_names` 只读列举（`list_branches`），无写路径。
- `list_branches` 已把 tags 快照落库（`src-tauri/src/commands/branch.rs:32-42`），数据侧已有基础。
  —— **漂移后仍成立**：GF-07 头部注释增长使行号漂到 `:25-46`，`dao::replace_tags(...)`
    落在 `:45`；数据侧（DB tags 表）确实已有快照，缺的只是写操作与 UI 入口。

补充核验（设计依据，非静态证据）：

- `git push origin <tag>` **不会**创建 `refs/remotes/origin/tags/<tag>`（实测 git 2.4x：
  推完 tag 后 `git for-each-ref refs/remotes` 只有 master），因此「标签是否已推送」不能
  只靠本地 remote-tracking refs 判定——需 `git ls-remote --tags`，失败时才回退本地 refs。
- `git push origin <tag>` 对已存在标签：普通 push 与 `--force-with-lease` 均拒绝
  （`already exists` / `stale info`），只有 `--force` 覆盖；force 类参数确属用户显式选择。

## 修复范围 checklist

- [x] 1. 后端 `branch.rs` 新增 tag 命令：`create_tag`（name + 可选 message + 可选目标 oid，缺省 HEAD）、`delete_tag`、`push_tag`（含 force 参数，默认禁用）。
- [x] 2. 前端 Tags 行加 `n-dropdown` 动作菜单（放在现空占位处）：创建标签（面板级按钮）、推送、删除。
- [x] 3. 删除按 Roadmap §46 危险分级走二次确认（Dangerous：删除已推送标签需明示）；force push 默认关闭并提示 "This may overwrite remote history."，推荐 `--force-with-lease`。
- [x] 4. tag 操作后随 `runOp` 统一刷新（引用 `BranchManager.vue:829-837` 的成功/失败模式）。

## 不做（范围控制）

- 不做标签排序/过滤/分组等列表增强。
- 不做「检出 tag」（游离 HEAD 检出属高危低频操作，另立任务评估）。

## 验收标准

1. 能在分支页对当前 HEAD 创建带注释标签，列表即时刷新。
2. 推送标签到远程成功；删除本地标签有确认且本地列表刷新。
3. 删除已推送标签 / force push 的确认流程符合 Roadmap §46。
4. `cargo test --lib`（GW_TEST_MANIFEST=1）与 `pnpm build` 通过。

## 实现说明（GF-04 落地）

后端（`src-tauri/`）：

| 位置 | 内容 |
|---|---|
| `core/branch.rs` | `create_tag`（message 非空 → 附注标签，否则轻量；`target` 缺省 HEAD；已存在名拒绝）、`delete_tag`（仅本地 `refs/tags/*`）、`tag_in_remote_tracking_refs`（离线回退判定）、`validate_tag_name`（拒前导 `-` 等，避免被 `git push` 当选项解析）。均本地 libgit2 操作，无网络。 |
| `core/git_ops/remote.rs` | `push_tag_streaming`（`git push <remote> [--force\|--force-with-lease] <tag>`，标签不存在先报 NotFound）、`remote_tags_streaming`（`git ls-remote --tags <remote>`）。两者复用 `run_git_streaming`（`spawn_streaming`：`CREATE_NO_WINDOW`、取消、300s 超时杀进程树），不新造 spawn。 |
| `commands/branch.rs` | `create_tag` / `delete_tag`（同步，纯本地，同分支变体）+ `push_tag` / `tag_pushed_to_remote`（**async + `spawn_blocking` + `ConsoleStreamer` 流式镜像 + `register_single_op`/`emit_op_finished`**，与 GF-07 `push_branch` 同一模式，遵守 F-43：网络操作不占 IPC 线程）。`parse_remote_tag_names` 跳过 `^{}` peeled 行。 |
| `lib.rs` | invoke_handler 注册上述 4 个命令。 |

新增单测（4 项，全绿）：

- `core/branch.rs`：`tag_create_and_delete_lifecycle`（附注/轻量、重复与未知 target 拒绝、
  前导 `-` 拒绝、删除后列表变化、删不存在报错）、`tag_in_remote_tracking_refs_detects_fetched_tags`。
- `commands/branch.rs`：`parse_remote_tag_names_skips_peeled_refs`；
  `tag_push_round_trip_against_local_remote`（真 git CLI + 本地 bare remote 往返：
  新标签 push 成功 → 本地无该标签快速失败 → `ls-remote` 列名 → plain push 与
  `--force-with-lease` 拒绝 stale tag → `--force` 覆盖成功 → 本机推送不留
  remote-tracking ref；git 不在 PATH 时 skip 并打印原因）。

前端（`src/`）：

- `api/branch.ts`：`createTag` / `deleteTag` / `pushTag` / `tagPushedToRemote` wrapper。
- `views/BranchManager.vue`：
  - Tags 面板 `Panel #actions` 加「新建标签」按钮；行尾空 `<span />` 换成 `n-dropdown`
    （Push… / Delete，Delete 用 `--gw-danger` 着色）。
  - 新建标签 modal：标签名（字符集校验，非法时 `status="error"` + 按钮禁用）+ 附注消息
    （留空 = 轻量标签）+ 目标提示（当前 HEAD + oid）。
  - Push 标签 modal：force 默认关闭（§47）；勾选后出现 `--force-with-lease`（默认，推荐）
    / `--force` 单选与 "This may overwrite remote history." 警示。
  - Delete：先 `tagPushedTo_remote`（`git ls-remote --tags`，查询期间 `message.loading`），
    已推送时弹窗明示「此标签已推送到远程仓库——删除本地不会删除远程副本」，再二次确认。
  - 增 / 删 / 推 均走现有 `runOp`（成功 toast + `load()` 刷新，失败 toast 不刷新）。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（前端只读孤岛 + 后端无 tag 命令，双缺口）。待复现与修复。 |
| 2026-09-24 | 复现核验：三条定位线索全部成立（`:141` 空 `<span />` 无漂移；9 命令无 tag；`replace_tags` 行号 32-42→45 漂移但仍在）。另实测两条设计依据：tag push 不产生 remote-tracking ref（"已推送"判定需 `ls-remote`）、git 默认拒绝覆盖远程已有标签（force 属显式选择）。 |
| 2026-09-24 | 修复完成：根因 = 后端无 tag 变更命令 + 前端只剩空占位（双缺口）。修法 = 见「实现说明」表（本地 create/delete 走 libgit2，network 走 async 流式 CLI；UI 走 naive-ui + `--gw-*` tokens，删除/force 按 §46/§47 二次确认）。 |

**验证记录（2026-09-24）**：

| 命令 | 结果 |
|---|---|
| `GW_TEST_MANIFEST=1 cargo test --lib` | **965 passed / 17 failed / 3 ignored**。17 项失败与环境 HEAD 基线逐条比对后全部为预存在的环境/负载抖动，无一落在 branch/tag 代码：real_maven×10、real_node_vite×1、node::workspace×2、pty smoke×1、pathutil（大小写不敏感 FS）×1、benchmark runtime smoke×1、ai::gateway 超时×1。 |
| 基线比对方法 | `git archive HEAD src-tauri` 抽出 HEAD 树到仓外临时目录另跑一遍（951 passed / 18 failed）。基线独有失败 `models::ipc_golden::ts_types_match_rust_samples` 是**抽取伪影**（该测试读 `<repo>/src/types/*.ts`，最小抽取里没有 `src/`；本工作区该测试通过）；基线与本次各自出现的 runtime service/scheduler 并发上限失败与 ai 超时属负载抖动，单独重跑均通过。 |
| `pnpm build`（`vue-tsc --noEmit && vite build`） | 通过（BranchManager 36.93 kB）。 |

diff 形状：`commands/branch.rs` +316/-0、`core/branch.rs` +157/-0、`core/git_ops/remote.rs` +61/-0、
`lib.rs` +7/-0、`api/branch.ts` +62/-0、`BranchManager.vue` +250/-3（3 行删除 = 空 `<span />`
占位 + 两行 import 重排）——**纯增量，未改任何既有符号逻辑**。
