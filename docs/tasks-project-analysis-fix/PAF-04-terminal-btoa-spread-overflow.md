# PAF-04 终端 btoa spread 大文本栈溢出（3 处）

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P0-4，主控 + 核查智能体双重验证 |
| 关联任务 | TM-02（终端面板） |

## 问题描述

三处 `btoa(String.fromCharCode(...bytes))` spread 写法：

- `src/components/terminal/XtermView.vue:114`（onData 输入）
- `src/components/terminal/TerminalPanel.vue:149`（右键粘贴）
- `src/commands/registry.ts:270`（命令面板粘贴）

粘贴大段文本（几万字节以上）时 spread 展开超引擎参数上限，抛
`RangeError: Maximum call stack size exceeded` 且未捕获，输入监听中断，
终端看似「卡死无响应」。

## 定位与修复建议

- 抽出公共分块 base64 编码工具（循环切片 8KB 编码拼接），三处统一替换。
- `stores/terminal.ts` 的 atob 循环解码方向是安全的，无需改动。

## 验收标准

- [ ] 粘贴 1MB 文本到终端不报错、内容完整
- [ ] 三处调用点统一走分块实现
- [ ] `pnpm build` 通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
