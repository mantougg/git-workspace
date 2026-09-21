# F-55 检查更新后「下载安装」报错：Cannot read private member from an object whose class did not declare it

> 状态：✅ 已完成
> 优先级：P0（更新通道完全不可用：能检测、无法下载安装）
> 来源：2026-09-21 用户实测反馈。

## 问题描述

关于页点击「检查更新」可正常发现新版本，点击「下载安装」立即报错：

```
Cannot read private member from an object whose class did not declare it
```

## 根因定位（已核实，2026-09-21）

`src/composables/useUpdater.ts:30` 用 `ref<Update | null>(null)` 持有
`@tauri-apps/plugin-updater` 的 `Update` 实例。`ref()` 对赋值对象做**深度响应式
包裹（Proxy）**；而 `Update` 继承自 `@tauri-apps/api` 的 `Resource` 基类，其
`rid` 存在 **WeakMap 私有字段**（`core.js: _Resource_rid = new WeakMap()`，
`get rid()` 经 `__classPrivateFieldGet(this, _Resource_rid)` 读取）。

`downloadAndInstall()` / `close()` 内部访问 `this.rid` 时，`this` 是 Vue Proxy
而非原实例 → WeakMap 查不到 → 抛 "Cannot read private member …"。

「检查更新」不报错的原因：`check()` 后只读取 `version`/`body` 公共字段，
Proxy 透明转发；只有调用方法（触发私有字段访问）才炸。`closePendingUpdate`
里的 `close()` 同样会炸，但被 `.catch(() => undefined)` 静默吞掉。

## 修复范围 checklist

- [x] 1. `useUpdater.ts`：`ref` → `shallowRef` 持有 Update 实例（浅响应式不包裹
  类实例；status 等标量 ref 不变）。

## 不做（范围控制）

- 不改 AboutView 的交互与错误展示（现有 `error.value` 展示链路正常）。
- 不引入前端测试基建（项目无 vitest；该缺陷依赖 Vue reactivity + 插件类，
  验证方式为构建 + 实机回归）。

## 验收标准

1. 检查更新发现新版本后，点击「下载安装」正常进入下载进度，不再报私有字段错误。
2. 重复「检查更新」（先 close 旧 Update）与关闭页面（onBeforeUnmount close）不报错。
3. `pnpm build`（vue-tsc + vite）通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-21 | 开始修复。已定位：useUpdater.ts:30 `ref` 深包裹 Update 实例 → Resource 的 WeakMap 私有字段 `rid` 在 Proxy 上读取失败。修法：`ref` → `shallowRef`。 |
| 2026-09-21 | 修复完成。`pnpm build`（vue-tsc+vite）绿。同类隐患排查：`reactive(new Map())` 为 Vue 官方支持的集合适配，无其他类实例深响应式持有点。实机回归：检查更新 → 下载安装 → 重启链路待用户复测。 |
