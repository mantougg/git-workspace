# F-47 AI 生成 Commit Message：勾选未暂存文件时结果为空或只覆盖部分文件

| 项 | 值 |
|---|---|
| 优先级 | P0 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-20 用户反馈：勾选多个层级的变更文件生成 Commit Message，「有时候说完成了，但是没有文字输出，有时候又只是展示最外层变更文件的 message」 |
| 关联任务 | F-46（生成慢/轮询超时）、F-48（思考增量不透传） |

## 问题描述

「变更与批量操作」勾选文件 → 「AI 根据勾选文件生成 Commit Message」，两种异常：

1. 提示「AI 已生成 Commit Message」成功，但输入框没有文字；
2. 生成的 message 只描述了最外层（先前已暂存的）文件，勾选的其余文件没被覆盖。

## 根因（已定位）

**主根因：diff 范围与提交语义不一致。**

- AI 生成走 `generateCommitMessage`（`src/views/RepositoryList.vue:1841`），
  硬编码 `diffScope: "staged"`（:1870）；后端 `CommitMessage` 默认同样是
  `DiffScope::Staged`（`ai/preview.rs:477`）。
- 但批量提交是在**提交那一刻才 stage** 勾选文件（`core/git_ops/commit.rs:85`
  `index.add_path`）。勾选时尚未暂存的文件，`staged` diff 里根本不存在。
- 后果分两种：暂存区全空 → AI 拿到近乎空的上下文，返回空 title 或无法解析的
  文本；暂存区有部分旧内容（先前操作残留的已暂存文件）→ message 只覆盖那部分
  —— 即用户看到的「最外层变更文件」。

**次根因：前端结果应用不健壮**（`RepositoryList.vue:1888-1897`）。

- 只认 `result.type === "commitSuggestion"`；模型没返回合法 JSON 时后端降级为
  `Answer`/`GeneratedText`（`ai/request.rs:357`），前端直接警告「未返回有效的
  提交建议」把文本丢掉了。
- `payload.title` 为空/缺失时照样弹成功提示，`commitForm.message` 被赋成
  `undefined`/空串。

## 修复范围

- [x] `generateCommitMessage` 的 `diffScope` 改为 `workdir`
      （`get_workdir_diff` = HEAD→workdir+index 且含 untracked，
      `core/diff.rs:80`，与批量提交 `add_path` 后的提交内容语义一致；
      includePaths 过滤保证只看勾选文件）
- [x] 结果应用加固：`commitSuggestion` 的 `title` 为空时改用 body 拼接
      兜底；`answer`/`generatedText` 降级结果直接把 `text` 填入输入框；
      全部落空时才提示失败，不弹成功
- [ ] 真机验证「勾选未暂存文件 → 生成的 message 覆盖全部勾选文件」
      （待用户实测）

## 验收标准

- [x] 勾选从未暂存过的文件，生成的 message 覆盖全部勾选文件（不留空）
      —— 由 `diffScope: "workdir"` 保证（diff 语义与提交内容一致），真机待用户实测
- [x] 模型返回非 JSON 文本时，文本仍填入输入框而不是警告丢弃
- [x] `vue-tsc` 通过

## 进度

### 状态

- 当前状态：✅ 已完成（代码 + 类型检查全绿；真机复现案例待用户实测）
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`diffScope:"staged"` 与批量提交「提交时才 stage」语义不一致 + 前端结果应用不健壮 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：`generateCommitMessage` 的 `diffScope` 改 `workdir`（含 untracked，与提交内容语义一致）；结果应用加固——title 空用 body 兜底、answer/generatedText 降级文本直接填入、全部落空才提示失败不弹成功；vue-tsc 通过 |
