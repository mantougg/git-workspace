# D-09 等宽字体栈接入

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §3.1（`--gw-font-mono`）/ §5.0-5（通用规则 5）；直接依赖：[D-08](./D-08-token-migration.md)。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Desktop Visual System |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | D-08 |
| 对应方案 | §3.1 / §5.0 |

## 目标

路径、分支名、commit hash、日志、diff 内容等技术性文本统一使用等宽字体栈，强化「开发者工具」观感。

## 需求范围

- [ ] 定义通用工具类（如 `.mono { font-family: var(--gw-font-mono); }`）或按组件 scoped 接入
- [ ] 接入点：仓库/文件路径（RepositoryList diff-pane-header、GitGraph repo-path、任务型页面标题栏）、分支名（branch-bar、分支管理）、commit hash、操作日志、Runtime 日志视图、DiffViewer 与冲突解决器的代码区
- [ ] 中文混排目检：等宽栈只作用于技术性文本，不误伤正文

## 验收标准

- [ ] 上述接入点全部使用 `--gw-font-mono`
- [ ] 亮/暗下显示正常，对齐无异常
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-08-27 任务拆解录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 二期-3） |
