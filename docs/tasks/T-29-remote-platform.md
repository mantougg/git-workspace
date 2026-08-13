# T-29 Remote Platform 集成 + Pull Request + CI

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-11 Commit 增强](./T-11-commit-enhance.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 5 · Remote Platform（P2） |
| 优先级 | P2 |
| 状态 | ⬜ 未开始 |
| 依赖 | T-09, T-11 |
| 对应 Roadmap | §27 GitHub/GitLab/Gitea 集成、§28 Pull Request |

## 目标

接入远程平台（GitHub / GitLab / Gitea / Gitee / Bitbucket），提供 Open Repository/Issue/PR、Create Pull Request、View CI 等能力。

## 需求范围

- [ ] 平台抽象层：GitHub / GitLab / Gitea / Gitee / Bitbucket
- [ ] Open Repository / Open Issue / Open Pull Request / View CI
- [ ] Create Pull Request：Source/Target 选择、Commits/Files 统计、AI 生成 Title/Description（复用 T-27）
- [ ] 平台凭据：优先系统 git 凭据 / 已有 token，敏感信息走 T-08 保护与 OS Credential Store（§69）

## 架构 / 性能注意点

- 平台调用走异步 HTTP 客户端，失败可重试，不阻塞 UI；速率限制友好。
- 凭据管理优先复用系统 git / OS 凭据，不自行实现复杂 credential storage。

## 验收标准

- [ ] 至少 GitHub / GitLab 完整可用（Open + Create PR + View CI）
- [ ] Create PR 正确回填 Source/Target/Commits/Files 与 AI 描述
- [ ] 平台 token 不落盘明文，走 OS Credential Store

## 进度

### 状态

- 当前状态：未开始
- 最近更新：—

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| — | — | — |

### 子任务清单

- [ ] 平台抽象与 GitHub/GitLab 实现
- [ ] Open Repository/Issue/PR/CI
- [ ] Create PR 流程
- [ ] 凭据与 OS Credential Store
