# F-18 Change Set 页空状态未占满（n-spin 容器不参与 flex 布局）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-08-28 用户实测反馈问题 2 |
| 关联任务 | F-09b（同类 n-spin 布局问题）、F-13 |

## 问题描述

Change Set 页面在没有数据时，空状态（插图 + 「新建 Change Set」按钮）
没有占满可用区域，而是挤在左上角。

## 根因（已定位）

`ChangeSetView.vue` 的 `.main-body` 是 `display: flex` 行布局，但它的
直接子元素是两个 `n-spin`——Naive UI 将其渲染为 `.n-spin-container`
（`display: block`、无高度、不参与 flex 宽度分配）。被包在里面的
`.set-list`（`width: 280px`）和 `.set-detail`（`flex: 1`）因此：

- `.set-detail` 的 `flex: 1` 失效（父容器不是 flex），右侧详情区横向
  不扩展；
- 两个区块高度都塌陷为内容高度，`n-empty` 自然停在左上角。

同类问题在 F-09b（diff 面板）已踩过一次：n-spin 必须显式参与 flex
布局，不能只包内容。

## 修复范围

- [x] 两个 `n-spin` 增加专用 class，样式上参与 `.main-body` 的 flex
  布局（左侧固定 280px、右侧 `flex: 1; min-width: 0`，高度 100%，
  `.n-spin-content` 高度链打通）
- [x] `.set-list` / `.set-detail` 高度 100%
- [x] 空状态（左侧列表 `n-empty`、右侧详情 `n-empty`）在各自区域
  水平垂直居中

## 验收标准

- [x] 无 Change Set 时，左侧空状态在列表区居中展示（含新建按钮），
  右侧详情区空状态同样居中且区域占满
- [x] 有数据时列表/详情布局与之前一致，不出现双向滚动条或塌陷
- [x] loading 时 spin 遮罩覆盖各自区域

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 验收完成，Change Set 两侧 flex 高度链和空状态布局已验证

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-28 | ⬜ | 问题录入；定位：n-spin 渲染的 .n-spin-container 是 .main-body 的直接 flex 子项但未参与布局，内部 flex:1 失效、高度塌陷 |
| 2026-08-28 | 🟦 | 开始修复 |
| 2026-09-02 | ✅ | 验收完成：两个 n-spin 显式参与 flex，内部高度链打通，左右空状态居中；验证 pnpm build 通过 |
