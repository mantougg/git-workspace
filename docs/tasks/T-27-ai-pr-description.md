# T-27 AI PR Description + Security Review / Bug Detection / Commit Explanation

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)、[T-08 错误处理 + 日志 + Secret Protection](./T-08-errors-logging-secrets.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · AI Git Assistant（P1） |
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 依赖 | T-04, T-08 |
| 对应 Roadmap | §22 AI Git Assistant、§28 Pull Request |

## 目标

补齐 AI Git Assistant 的其余同构能力：PR Description 生成、AI Security Review、Bug Detection、Commit/File Explanation。均为「建议类」能力，复用同一调用链与 Secret 防护。

## 需求范围

- [x] PR Description：Title / Description / Summary / Testing / Risk 自动生成
- [x] AI Security Review：对 diff 做安全审查（注入/凭据/路径等）
- [x] Bug Detection：对 diff 做缺陷检测
- [x] Commit Explanation / File Explanation：解释某 commit / 文件意图
- [x] 统一输出为建议卡片，用户可复制/采纳，不自动写文件
- [x] 全部走 Secret 检测 + Preview + Exclude（T-08）

## 架构 / 性能注意点

- 这些能力与现有 AI Review 同构，抽公共「diff → prompt → 建议」管道，避免重复实现请求/截断/重试逻辑。
- 生成结果仅展示，不自动修改文件或提交（AI as Assistant 原则）。

## 验收标准

- [x] 四类能力均可用且输出为建议卡片
- [x] PR Description 生成内容覆盖 Title/Summary/Testing/Risk
- [x] 安全审查与缺陷检测能标注到具体文件/行
- [x] 全部能力接入 Secret 检测与 Preview

## 进度

### 状态

- 当前状态：已完成
- 最近更新：2026-08-30

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-08-30 | 开始开发 | 由 AI-08 承载 PR Description、Security Review、Bug Detection 与 Explanation 场景实现。 |
| 2026-08-30 | 完成 | 由 AI-08 实现：PR Description、Security Review、Bug Detection、Commit/File Explanation 均复用 Preview、Secret、排除、缓存与结构化结果链路；提交解释使用 T-12 的只读 Commit Diff。验证见 AI-08。 |

### 子任务清单

- [x] 公共「diff → 建议」管道抽取
- [x] PR Description 生成
- [x] Security Review / Bug Detection
- [x] Commit / File Explanation
