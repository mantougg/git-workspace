# T-19 Workspace Health

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-02 Status Engine](./T-02-status-engine.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 2 · Multi-Repo Engine（P1） |
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-02 |
| 对应 Roadmap | §19 Workspace Health |

## 目标

实现 Workspace 健康检测与健康评分，自动发现脏/冲突/超前/落后/游离/缺远程/分歧等异常。

## 需求范围

- [ ] 检测项：Dirty / Conflict / Ahead / Behind / Detached / Missing Remote / Diverged / Untracked / Large Files / LFS Error / Submodule Error
- [ ] 健康评分：0~100%，汇总展示（如 91%）
- [ ] 每项异常可下钻到具体仓库列表
- [ ] 异常仓库排序与快速筛选
- [ ] 数据基于 T-02 状态缓存，无额外网络/扫描开销

## 架构 / 性能注意点

- 评分规则可配置（各项权重），放配置文件而非硬编码。
- Large Files / LFS / Submodule 检测属于重项，按需（进入 Health 页时）异步计算，不进常驻状态路径。

## 验收标准

- [ ] 各类异常正确识别并可下钻定位仓库
- [ ] 评分计算规则清晰且可配置
- [ ] 健康页打开不阻塞 UI（重项异步）

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 检测项实现与评分规则
- [ ] Health 页 UI（评分 + 异常下钻）
- [ ] 异常筛选联动
