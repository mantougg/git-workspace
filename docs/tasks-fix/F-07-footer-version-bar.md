# F-07 应用底部版本与作者栏 + AGENTS.md 规则

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | 🟦 修复中 |
| 来源 | 2026-08-27 用户反馈问题 7 |

## 问题描述

在应用底部增加一栏，展示作者与版本说明，格式示例：`v0.1.0 by mantougg`。

- `v0.1.0`：当前版本号（与 `package.json` / `tauri.conf.json` 版本保持一致，取单一数据源，不要两处手写）
- `mantougg`：开发者名字

同时需要把「底部展示 版本号 by 作者」作为一条规则写入根目录 `AGENTS.md`，让后续开发遵守。

## 修复范围

- [x] 全局布局底部增加版本栏，所有页面可见
- [x] 版本号从配置单一数据源读取（构建期注入或运行时读取，避免硬编码漂移）
- [x] 作者名可配置
- [x] 根目录 `AGENTS.md` 增加此规则说明

## 验收标准

- [x] 应用底部常驻展示 `vX.Y.Z by <author>`，版本号与实际发布版本一致
- [x] `AGENTS.md` 中新增对应规则条目
- [x] 底部栏不遮挡既有内容、不影响既有布局交互

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-27 完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-27 | ⬜ | 问题录入（拆分自用户反馈） |
| 2026-08-27 | 🟦 | 开始修复 |
| 2026-08-27 | ✅ | 完成：`package.json` 补 `author: mantougg`（与 `version` 共同作为唯一数据源）→ `vite.config.ts` `define` 构建期注入 `__APP_VERSION__`/`__APP_AUTHOR__`（声明在 `src/vite-env.d.ts`）→ `App.vue` 底部新增 `.app-footer` 常驻栏（页面区包一层 `.view-area` flex:1 min-height:0，既有 `height:100%` 页面不受影响；TaskPanel 是 n-drawer 悬浮不参与布局）；注意点：define 常量不能直接写进模板（会编成 `_ctx.` 属性访问），需在 script 里赋值给局部 const。`AGENTS.md` 新增「应用底部版本栏规则（F-07）」段落。验证=`pnpm build` 通过，产物 index chunk 含 `0.1.0`/`mantougg`/`app-footer` |

### 子任务清单

- [x] 底部版本栏 UI + 版本数据源
- [x] AGENTS.md 规则补充
