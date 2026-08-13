# T-27 AI PR Description + Security Review / Bug Detection / Commit Explanation

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-04 Diff & Graph 硬化](./T-04-diff-graph.md)、[T-08 错误处理 + 日志 + Secret Protection](./T-08-errors-logging-secrets.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 3 · AI Git Assistant（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-04, T-08 |
| 对应 Roadmap | §22 AI Git Assistant、§28 Pull Request |

## 目标

补齐 AI Git Assistant 的其余同构能力：PR Description 生成、AI Security Review、Bug Detection、Commit/File Explanation。均为「建议类」能力，复用同一调用链与 Secret 防护。

## 需求范围

- [ ] PR Description：Title / Description / Summary / Testing / Risk 自动生成
- [ ] AI Security Review：对 diff 做安全审查（注入/凭据/路径等）
- [ ] Bug Detection：对 diff 做缺陷检测
- [ ] Commit Explanation / File Explanation：解释某 commit / 文件意图
- [ ] 统一输出为建议卡片，用户可复制/采纳，不自动写文件
- [ ] 全部走 Secret 检测 + Preview + Exclude（T-08）

## 架构 / 性能注意点

- 这些能力与现有 AI Review 同构，抽公共「diff → prompt → 建议」管道，避免重复实现请求/截断/重试逻辑。
- 生成结果仅展示，不自动修改文件或提交（AI as Assistant 原则）。

## 验收标准

- [ ] 四类能力均可用且输出为建议卡片
- [ ] PR Description 生成内容覆盖 Title/Summary/Testing/Risk
- [ ] 安全审查与缺陷检测能标注到具体文件/行
- [ ] 全部能力接入 Secret 检测与 Preview

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 公共「diff → 建议」管道抽取
- [ ] PR Description 生成
- [ ] Security Review / Bug Detection
- [ ] Commit / File Explanation
