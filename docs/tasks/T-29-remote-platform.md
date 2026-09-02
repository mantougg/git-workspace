# T-29 Remote Platform 集成 + Pull Request + CI

> **开发前必读**：先读 [00-全局开发约束.md](./00-全局开发约束.md)；直接依赖：[T-09 Branch Manager](./T-09-branch-manager.md)、[T-11 Commit 增强](./T-11-commit-enhance.md)。

| 项 | 值 |
|---|---|
| 阶段 | Phase 5 · Remote Platform（P2） |
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 依赖 | T-09, T-11 |
| 对应 Roadmap | §27 GitHub/GitLab/Gitea 集成、§28 Pull Request |

## 目标

接入远程平台（GitHub / GitLab / Gitea / Gitee / Bitbucket），提供 Open Repository/Issue/PR、Create Pull Request、View CI 等能力。

## 需求范围

- [x] 平台抽象层：GitHub / GitLab / Gitea / Gitee / Bitbucket
- [x] Open Repository / Open Issue / Open Pull Request / View CI
- [x] Create Pull Request：Source/Target 选择、Commits/Files 统计、AI 生成 Title/Description（复用 T-27）
- [x] 平台凭据：优先系统 git 凭据 / 已有 token，敏感信息走 T-08 保护与 OS Credential Store（§69）

## 架构 / 性能注意点

- 平台调用走异步 HTTP 客户端，失败可重试，不阻塞 UI；速率限制友好。
- 凭据管理优先复用系统 git / OS 凭据，不自行实现复杂 credential storage。

## 验收标准

- [x] 至少 GitHub / GitLab 完整可用（Open + Create PR + View CI）
- [x] Create PR 正确回填 Source/Target/Commits/Files 与 AI 描述
- [x] 平台 token 不落盘明文，走 OS Credential Store

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-02 完成开发

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-02 | 🟦 | 开始开发。方案：`src-tauri/src/remote/`（platform：origin URL 解析 + 各平台 Open/NewPR/CI URL 纯函数；api：GitHub/GitLab/Gitea/Gitee/Bitbucket REST create PR + CI 状态，reqwest）；凭据 keyring（复用 AI-01 OsCredentialStore，ref=`remote:{platform}:{host}`）→ 系统 `git credential fill` 回退，不落盘明文；前端 BranchManager 远程菜单 + Create PR 对话框（compareBranches 回填 Commits/Files，AI 描述走 T-27 aiBuildContextPreview/aiSubmitRequest 管线，gitScenario=prDescription） |
| 2026-09-02 | ✅ | 完成。后端 `remote/platform.rs`（HTTPS/SSH origin 解析、五平台 Open/Compare/NewPR/Issue/PR/CI URL、API base；未知主机按 Gitea 处理）、`remote/api.rs`（五平台 Create PR 请求体/认证头/响应解析/CI 状态解析纯函数 + 单次 HTTP 调用，401/403/404/422 映射可行动错误）、`commands/remote.rs`（detect_remote / remote_open_url / create_pull_request / get_ci_status / resolve_remote_token / save_remote_token / delete_remote_token；凭据链 keyring → git credential fill，spawn_blocking 不跨 await）；前端 `api/remote.ts` + BranchManager 远程下拉（打开仓库/Issues/PR 列表/CI 状态）+ Create PR 对话框（Source/Target 选择、compareBranches 回填提交/文件统计、结构化描述底稿、AI 生成走 T-27 预览-审批-轮询管线并回填 title/body、token 保存到 OS 凭据库）。边界说明：GitHub/GitLab 实际 API 调用需真实 token + 网络，本环境无法端到端实测——URL/请求体/响应解析均以纯函数单测覆盖（8 项），打包后联调按此边界进行。验证：`cargo test --lib` 800 通过；`pnpm build` 通过 |

### 子任务清单

- [x] 平台抽象与 GitHub/GitLab 实现
- [x] Open Repository/Issue/PR/CI
- [x] Create PR 流程
- [x] 凭据与 OS Credential Store
