# GF-22 ConflictResolver「AI 冲突建议」disabled 死按钮（T-26 已交付）

> 状态：⬜ 未开始
> 优先级：P2
> 来源：GF-06 复现时牵出（2026-09-24）。

## 问题描述

`src/views/ConflictResolver.vue:18` 的「AI 冲突建议」按钮 `disabled` 且 title 写
「AI 冲突建议将在 T-26 提供」——但 T-26 已交付：同文件 `:90` 正在渲染
`AiConflictAssistant` 组件（rc/hidden 按钮形态）。一个已存在的功能入口被标为
「未来提供」，用户绕路去终端手动解决。

## 定位线索（证据）

- disabled 按钮 + 死文案：`src/views/ConflictResolver.vue:18`（GF-06 复现时核对）
- 已交付组件：`src/views/ConflictResolver.vue:90`（`AiConflictAssistant`，内容/候选
  → RESULT 编辑器的流程已实现）
- 组件本体：`src/components/ai/AiConflictAssistant.vue`

## 待核验（复现优先）

1. 按钮 disabled 的真实前置条件是什么（需先选中冲突文件？AI 未配置？）？
2. 按钮与已在渲染的 AiConflictAssistant 面板是什么关系（两个入口还是一个
   已废弃的重复入口）？
3. AI 未配置时的引导是否与设置页打通（AI 402/未配置类错误的现有处理模式）？

## 修复范围 checklist

- [ ] 1. 核验前置条件与入口关系。
- [ ] 2. 恢复按钮可用或删除重复入口（与现有 AiConflictAssistant 统一）。
- [ ] 3. 清理「T-26 提供」死文案。

## 验收标准

1. 选中冲突文件时 AI 建议入口可达（走现有 AiConflictAssistant 流程）。
2. 前置条件不满足时提示真实原因（含 AI 未配置引导）。
3. `pnpm build` 通过。

## 进度

| 日期 | 记录 |
|---|---|
| 2026-09-24 | 建任务：GF-06 复现确认 T-26 已交付（AiConflictAssistant 在同文件渲染中），旧入口仍 disabled。待核验与修复。 |
