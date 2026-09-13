# PAF-20 MCP 本地端点无鉴权（per-boot token + 请求头校验）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ⬜ 未开始 |
| 来源 | 项目全景分析报告（docs/project-analysis-2026-09-13.md）P1-23，主控亲自验证 |
| 关联任务 | AI-12（外部 Agent 适配器）、T-08（Secret 防护） |

## 问题描述

`src-tauri/src/ai/external/server.rs`：本地 HTTP 端点（默认端口 39117，
:29，占用回退临时端口）**无任何鉴权**——无 token，`read_request` 不校验
Content-Type/Origin/Host。本机任意进程可 POST 到 127.0.0.1:39117 读取全部
只读工具数据；更值得注意的是**浏览器 CSRF 可达**——恶意网页可用
`text/plain` 简单请求（免 preflight）发送 JSON-RPC，触发只读工具调用与
创建 pending Proposal（响应因 CORS 读不到，但副作用已发生）。discovery
文件 `ai-external-endpoint.json` 退出时清理（:264）。

## 定位与修复建议

- 生成 per-boot 随机 token 写入 discovery 文件，请求要求
  `Authorization: Bearer <token>`；
- 校验 `Content-Type: application/json`（阻断浏览器简单请求）与 Host/Origin；
- CLI（`git-workspace ai-tools call`）从 discovery 文件读取 token。

## 验收标准

- [ ] 无 token / 错误 token 请求返回 401
- [ ] text/plain 请求被拒绝
- [ ] 合法 CLI/外部 agent 调用不受影响
- [ ] `cargo test --lib`（ai::external）通过

## 进度

### 状态

- 当前状态：⬜ 未开始
- 最近更新：2026-09-13 录入

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
