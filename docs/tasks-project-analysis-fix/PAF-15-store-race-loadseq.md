# PAF-15 stores 过期响应覆盖与 runtime 双监听器竞态（loadSeq 模式推广）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-18/P1-19，核查智能体逐项验证 |
| 关联任务 | D-15、F-14/F-17（仓库解析链） |

## 问题描述

1. **runtime store 双监听器**：`src/stores/runtime.ts:268-334` subscribe 的
   幂等守卫 `unlisteners.length > 0` 在第一个 await 之前判断，而数组赋值
   在 12 个 `await listen` 全部完成后——快速进出 Runtime 视图产生两批
   监听器（日志行重复显示、事件双触发），前一批永不释放。
2. **过期响应覆盖族**（均无 seq/requestId 防护，快速切换时旧响应覆盖新
   数据）：
   - `stores/changeSet.ts:36-52`（selectChangeSet）
   - `stores/runtime.ts:92-126`（reloadAll/loadConfigs/loadProjects 完成时
     不校验 workspaceId）
   - `stores/repository.ts:39-62`（scanRepositories/loadRepositories）
   - `src/views/RepositoryList.vue:1531-1563`（双击文件 diff）
   - `src/views/RuntimeDependenciesView.vue:706-718`（onSelectProject）

   对照最佳实践：`DiffViewer.vue:369-395` 的 `loadSeq` 递增序号丢弃过期
   响应。

## 定位与修复建议

- subscribe 改为缓存 in-flight Promise（参照 terminal.ts listenersReady
  模式，但需带失败回滚，见 PAF-16）；
- 五处竞态统一引入 loadSeq（或 workspaceId 完成时校验）模式。

## 验收标准

- [ ] 快速切换工作区/仓库/文件，旧响应不再覆盖新数据
- [ ] 快速进出 Runtime 视图不产生重复日志与双份 IPC
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
