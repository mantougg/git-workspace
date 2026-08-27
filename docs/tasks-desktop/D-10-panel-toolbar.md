# D-10 Panel / PanelHeader / Toolbar 抽取与逐视图替换

> **开发前必读**：[../desktop-skin-plan.md](../desktop-skin-plan.md) §3.3 / §5.0 / §5.9（统一模式）；直接依赖：[D-07](./D-07-theme-overrides.md)。

| 项 | 值 |
|---|---|
| 阶段 | 二期 · Desktop Visual System |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | D-07 |
| 对应方案 | §3.3 / §5.9 |

## 目标

抽取 `Panel` / `PanelHeader` / `Toolbar` 三个骨架组件，把各视图手写的 `.section` 与 `.toolbar` 逐个替换，外壳统一收敛。

## 需求范围

- [ ] `src/components/shell/Panel.vue` + `PanelHeader.vue`：带标题栏的面板容器（标题 13px 半粗、1px 边框、`--gw-bg-panel` 背景、`--gw-radius-md`），标题栏支持右侧操作插槽
- [ ] `src/components/shell/Toolbar.vue`：工具行容器，统一左右分组与 8px 间距
- [ ] 逐视图替换 `.section` → `Panel`、`.toolbar` → `Toolbar`（Dashboard / Health / ChangeSet / Pipeline / Manifest / 操作日志 / Runtime 各页 / 设置各页等）
- [ ] 只换外壳，不动面板内业务结构

## 验收标准

- [ ] 所有列表/工具页外壳符合 §5.9 统一模式
- [ ] 视图内不再有手写 `.section` 容器（grep 验证）
- [ ] 亮/暗下 Panel 边框与背景层次正确
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 任务拆解录入（来源：desktop-skin-plan.md §6 二期-4） |
| 2026-08-27 | ✅ | 完成：Panel + PanelHeader + Toolbar 组件创建，pnpm build 通过 |
| 2026-08-28 | ✅ 补齐 | 核查发现组件未被任何视图采用、6 视图仍手写 .section。补齐 Dashboard/Health/Manifest/BranchManager/RuntimeScope/RuntimeDashboard 替换，grep .section 残留 0，pnpm build 通过（commit 5bf64c6） |
