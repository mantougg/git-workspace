# GF-11 git 长列表无虚拟滚动 + GitGraph「加载更多」全量重取（O(n²)）

> 状态：🟦 部分完成（核心范围 1/2/3 已修完并验证；任务 4 的分支列表 + ChangeSet 已修，
> 冲突/Reflog/Stash 列表所在文件（ConflictResolver/Reflog/StashManager.vue）属本轮
> 职责边界「不要动」清单，移交后续批次，见进度 2026-09-24 第二条）
> 优先级：P1
> 来源：2026-09-24 Git 使用体验全景盘点。

## 问题描述

两类叠加的性能问题：

1. **翻页成本 O(n²)**：GitGraph 的「加载更多」是 `get_commit_history(repoPath, prevCount + PAGE_SIZE)`
   ——每次翻页全量重拉并整体替换 `commits`。翻到第 k 页时，累计传输/解析量是 O(k²)。
2. **列表零虚拟滚动**：提交图（CommitGraph，每行 3~5 个 SVG line/path）、变更树
   （ChangeTree，主界面）、ChangeSet 列表、分支本地/远程列表、Reflog、Stash、
   冲突列表全是 `v-for` 直渲。千级提交 / 千文件工作区下明显卡顿。

项目里已有成熟虚拟列表组件（`src/components/common/VirtualList.vue`，diff 组件在用），
属「有轮子没用开」。

## 定位线索（证据）

> 2026-09-24 复现核验结论（逐条）：
> - 第一条「成立」：GitGraph.vue 旧 `:315` 确为 `getCommitHistory(repoPath, prevCount + PAGE_SIZE)`
>   全量重取 + 整体替换 `commits`，翻到第 k 页累计传输 O(k²)。
> - 第二条「成立」：CommitGraph.vue 旧 `:6-107` 为全量 `v-for` 直渲（每行 SVG line/path 若干）。
> - 第三条「成立」：ChangeTree.vue 旧 `:3-18` 的 `n-tree` 未开 `virtual-scroll`。
> - 第四条「成立」：ChangeSetView.vue `:33`（set-list v-for）、BranchManager.vue 旧 `:90/:120/:143`
>   （local/remote/tags 三组 v-for）等其余列表均为 v-for 直渲。
> - 第五条「成立」：graph.rs 旧 `:56-76` 为 SQLite commit 元数据缓存命中路径（命中免 `find_commit`）。
>   加 offset/limit 后该路径未劣化：`load_commit_history_cached` 签名不变（T-07 benchmark 零改动），
>   内部委托给同一 `load_commit_history_page` 实现（offset=0），缓存命中逻辑逐行保留。

- 全量重拉：`src/views/GitGraph.vue:315`（`prevCount + PAGE_SIZE`）
- CommitGraph 全量渲染：`src/components/graph/CommitGraph.vue:6-107`
- ChangeTree 无 virtual-scroll：`src/components/repo/ChangeTree.vue:3-18`（`n-tree` 未开 `virtual-scroll`）
- 其余全量 v-for：`src/views/ChangeSetView.vue:33`、`src/views/BranchManager.vue:90,:120,:137`、`src/views/Reflog.vue:22`、`src/views/StashManager.vue:27`、`src/views/ConflictResolver.vue:35,:64`
- 后端已有缓存基础：`src-tauri/src/commands/graph.rs:56-76`（SQLite commit 元数据缓存，命中免 `find_commit`）——加分页参数后缓存命中路径不受影响。

## 修复范围 checklist

- [x] 1. 后端 `get_commit_history` 增 `offset/limit`（或 cursor）分页参数；前端 GitGraph 翻页改增量追加。
- [x] 2. CommitGraph 接 VirtualList（固定行高，参照 diff 组件用法）。
- [x] 3. ChangeTree `n-tree` 开 `virtual-scroll`。
- [x] 4a. 分支列表（BranchManager 本地/远程/Tags 三组）接 VirtualList；ChangeSetView
      「添加仓库」n-data-table 开 `virtual-scroll`（千仓库工作区）。
- [ ] 4b. 冲突列表（ConflictResolver.vue）/ Reflog / StashManager 长列表虚拟化——
      **不在本轮职责边界**（三个文件在 GF-11 任务「不要动」清单内，另有并行任务在改），
      建议由后续批次处理；排序与理由见进度。

## 不做（范围控制）

- 不一次性重写所有列表（分批，先在进度文档记录排序与理由）。
- 不改提交图 lazy heap walk 算法本身（已优化过，见 project-analysis）。

## 验收标准

1. 千 commit 仓库：翻到第 10 页时历史请求的累计数据量与页数线性相关（DevTools Network 核对），DOM 行数恒定。
2. 主界面千文件变更树滚动流畅（帧率对比：修复前 `startFrameMeter` 采样 vs 修复后）。
3. 既有提交图/变更树相关单测无回归；`pnpm build` 通过。

## 验收核验记录（2026-09-24）

- 标准 1：代码审查——`GitGraph.loadMore` 为 `commits.value = [...commits.value, ...more]`（只追加不替换），
  每页请求 `getCommitHistory(repoPath, undefined, offset, PAGE_SIZE)` 只传本页窗口；后端单测
  `page_returns_window_without_overlap` 用 10 commit 仓库构造数据验证分页窗口无重叠、拼接=全量、末页截断。
  DOM 行数恒定由 VirtualList 固定窗口（视口 + 20 行 overscan）保证；无 DevTools 实测（需真实千 commit 仓库 +
  运行态），留待人工抽检。
- 标准 2：ChangeTree 已开 `virtual-scroll`（n-tree VVirtualList 定高窗口）；受控展开/勾选/右键/双击与
  虚拟化的兼容性经静态核验（naive-ui 树状态均为 key 级计算，不依赖 DOM 全量渲染）+ `pnpm build` 类型检查；
  无自动化帧率测试，按任务说明以构造大数据人工/静态核验替代。
- 标准 3：`cargo test --lib` 992 passed / 15 failed / 3 ignored，15 个失败全部命中既有环境依赖失败清单
  （real_maven×10、real_node_vite、pty smoke、node workspace×2、pathutil 大小写），提交图/变更树相关
  （core::graph、commands::graph、benchmark diff_graph smoke）全绿；`pnpm build` 通过。

## 改动清单（2026-09-24）

| 文件 | 改动 |
|---|---|
| `src-tauri/src/commands/graph.rs` | `get_commit_history` 增 `offset`/`limit` 可选参数（`max_count` 保留为兼容别名）；命令体拆出 `load_commit_history_page`（offset/limit 窗口 + 跳过前缀），`load_commit_history_cached` 原签名委托 offset=0；新增 4 个分页单测（legacy 参数映射 / 窗口无重叠 / 边界截断与空页 / 缓存命中一致性） |
| `src/api/graph.ts` | `getCommitHistory` 增 `offset`/`limit` 可选参数（可选值统一 `?? null`，同项目惯例） |
| `src/views/GitGraph.vue` | `loadMore` 改增量追加（只 push 不替换）+ 并发/到尾守卫；`loadHistory` 走 offset=0 首页；`refreshSeq` 参与 CommitGraph key（整页重载复位滚动）；高度链 `.graph-spin :deep(.n-spin-content)` + `.graph-body height:100%` 修复 |
| `src/components/graph/CommitGraph.vue` | 接 `VirtualList`（ROW_H=30 定高）；`reset-scroll-on-items-change=false` 保滚动；footer 显示「已加载 N 条」；`:deep` 覆盖 max-content 宽度防横向滚动 |
| `src/components/common/VirtualList.vue` | 仅加可选 prop `resetScrollOnItemsChange`（默认 true，diff 组件零回归） |
| `src/components/repo/ChangeTree.vue` | `n-tree` 开 `virtual-scroll`；`.tree` 定高、外层 `overflow:hidden`（树自身成滚动容器） |
| `src/views/BranchManager.vue` | 本地/远程/Tags 三组列表接 `VirtualList`（34px 定高行，容器高度按行数封顶，短列表无嵌套滚动） |
| `src/views/ChangeSetView.vue` | 添加仓库 `n-data-table` 开 `virtual-scroll` |

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：体验盘点发现（`:315` 全量重拉 + 全路径零虚拟滚动）。待复现与修复。 |
| 2026-09-24 | 修复完成（核心范围）。根因：① GitGraph 翻页 `getCommitHistory(repoPath, prevCount + PAGE_SIZE)` 全量重取+整体替换，第 k 页累计传输 O(k²)；② CommitGraph/ChangeTree/分支列表全量 v-for 直渲，千级数据 DOM 行数线性膨胀。修法：① 后端 `get_commit_history` 增 `offset/limit`（`maxCount` 保留为兼容别名，`limit` 优先；默认行为不变），命令体拆为 `load_commit_history_page(conn, repo, offset, limit)`，`load_commit_history_cached` 原签名委托 offset=0（T-07 benchmark 零改动，缓存命中路径不变）；前端 GitGraph `loadMore` 改增量追加（`[...old, ...page]`），`loadHistory` 走 offset=0 首页。② CommitGraph 接 `VirtualList`（ROW_H=30 定高；VirtualList 加可选 prop `resetScrollOnItemsChange`，默认 true，diff 组件零回归；GitGraph 高度链经 `.graph-spin :deep(.n-spin-content)` 贯通，整页重载用 `key=repoPath#refreshSeq` 重挂载复位滚动）；ChangeTree `n-tree` 开 `virtual-scroll`（F-09 受控 expandedKeys / 勾选 emitSelection / 右键 / 双击展开均为 key 级状态，与虚拟化兼容）；BranchManager 三组分支/Tags 列表接 VirtualList（32px 定高行、容器高度按行数封顶，短列表无嵌套滚动）；ChangeSetView 添加仓库 n-data-table 开 `virtual-scroll`。验证：`GW_TEST_MANIFEST=1 cargo test --lib` = 992 passed / 15 failed / 3 ignored，15 个失败全部命中既有环境依赖失败清单（real_maven×10、real_node_vite×1、pty smoke×1、node workspace×2、pathutil 大小写×1，均为未触及模块 + 环境原因），无新增失败；新增 4 个 graph 分页单测全绿（含 offset/limit 边界、缓存命中一致性、legacy 参数映射）；`pnpm build`（vue-tsc --noEmit + vite build）通过。剩余缺口见 checklist 4b。 |
| 2026-09-24 | 任务 4 优先级排序与理由（本轮执行到 4a）：分支列表（>100 分支仓库常见，且操作密集——每行 dropdown，直渲卡顿最可感知）→ 冲突列表 → ChangeSet（本轮附带：添加仓库表在千仓库工作区可达千行）→ Reflog/Stash（数据量通常最小）。冲突/Reflog/Stash 三处所在文件（ConflictResolver.vue / Reflog.vue / StashManager.vue）在 GF-11 职责边界「不要动」清单内，交由后续批次；另发现 RepositoryList 侧栏提交图预览面板（`.graph-pane-spin`）缺 `:deep(.n-spin-content){height:100%}` 高度链修复（同 F-18/F-20 模式已用于 tree/diff 面板），预览面板内提交图为内容高度+overflow:hidden 裁切、不可滚动——属既有问题，本次按边界未动 RepositoryList.vue，建议另立 GF 任务。 |
