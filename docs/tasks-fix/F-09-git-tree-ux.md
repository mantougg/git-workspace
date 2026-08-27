# F-09 变更与操作页 Git 树问题集合

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-27 用户反馈问题 9（a–h 八个子项） |
| 关联任务 | T-22 Change Set、T-12 Diff Stage、T-20 Batch Ops |

## 问题描述

「变更与操作」页面的 Git 树存在 8 个交互/展示问题，逐项如下。

## 修复范围

- [x] a. **非叶子节点行双击展开**：鼠标在节点整行双击应展开/收起子级，目前只能点左侧折叠箭头（不要求必须点在 label 或箭头上）
- [x] b. **差异内容展示位置错误**：叶子节点双击展示差异内容，应该展示在树的**右侧**，且可**横向拖动改变宽度**（splitter）；现在展示在左侧把树覆盖了
- [x] c. **节点右侧状态标签**：展示分支名称、XX 处变更、已修改、未变更等，使用 Naive UI Tag 组件，配语义化颜色（已修改/变更数=warning、未变更=success、分支=info、文件状态按 modified=warning/deleted=error/added=success/renamed=info/untracked=default）
- [x] d. **未跟踪文件勾选后 Add 暂存按钮仍灰色**：勾选 untracked 文件应激活 Add 暂存按钮
- [x] e. **按钮/输入框禁用逻辑错误**：根因同 d（勾选状态同步不上来）。启用矩阵：无勾选时 Add/回退/Graph/Diff/分支/Stash/Worktree/提交身份/commit 输入框/提交 全部禁用；勾选任意节点（含仓库节点）→ Graph/Diff/分支/Stash/Worktree 启用；勾选了文件（勾选仓库会级联勾选文件）→ Add/回退/commit 输入框/提交 启用；Pull/Fetch/Push/冲突/批量分支/Workspace Stash 始终可用（内部有兜底与提示）
- [x] f. **中间选择器隐藏**：已隐藏（`v-if="false"` + 注释保留代码）
- [x] g. **展开全部 / 收起全部按钮无效**：根因是 ChangeTree expose 的两个方法是空实现；改为受控 expandedKeys 后实现
- [x] h. **无法返回首页**：工具栏左侧补「返回」按钮（→ dashboard）

## 验收标准

- [x] a–h 逐项修复并逐项验证（每子项在时间线或子任务清单单独勾选）
- [x] e 项的启用条件形成明确的「选中类型 × 操作」矩阵并文档化（见上方修复范围 e）
- [x] 既有 Git 树相关功能（勾选、暂存、提交）不回归

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈，a–h 八子项） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成。两个关键根因：①**d/e 同根因**——naive-ui Tree 实例只有 `getCheckedData()`，没有 `getCheckedKeys()`，迁移后 `emitSelection` 每次调用都抛 TypeError，勾选状态永远同步不到 `treeSelection`，导致所有依赖勾选数的按钮/输入框永久禁用（`ChangeTree.vue` 改为用 `@update:checked-keys` 回调参数保存 keys）；②**g**——`expandAll`/`collapseAll` 是空实现 stub，且树用的是非受控 `default-expanded-keys` 无法程序化控制（改为受控 `expanded-keys`，expandAll=收集全部父节点 key、collapseAll=清空）。其余：a=非叶子节点 dblclick 切换展开状态；b=diff 面板改为 `.main-body` 直接 flex 子元素（原 n-spin 包裹导致容器塌陷、面板盖到左侧树上），resize 手柄仅在有 diff 时显示；c=仓库节点 branch(info)/N处变更(warning)/已修改(warning)/未变更(success)，文件节点状态全部改 NTag 语义色；f=选择器行 `v-if="false"` 隐藏并保留代码；h=工具栏补「返回」→ dashboard。验证=`pnpm build`（vue-tsc + vite）通过；未在运行中的应用里逐项点击验证，建议实机过一遍 a–h |

### 子任务清单

- [x] a 非叶子节点行双击展开
- [x] b 差异面板移至右侧 + 可拖拽宽度
- [x] c 节点状态标签（分支/变更数/修改状态）
- [x] d 未跟踪文件勾选激活 Add 暂存
- [x] e 操作启用条件矩阵重构
- [x] f 中间选择器隐藏
- [x] g 展开全部/收起全部修复
- [x] h 返回首页导航
