# PAF-20 MCP 本地端点无鉴权（per-boot token + 请求头校验）

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
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

- [x] 无 token / 错误 token 请求返回 401（`auth_rejects_missing_wrong_token_and_cross_origin`）
- [x] text/plain 请求被拒绝（415，阻断浏览器简单请求 CSRF）
- [x] 合法 CLI/外部 agent 调用不受影响（CLI 从 discovery 读 token 附 Authorization 头；既有 round-trip 测试更新后通过）
- [x] `cargo test --lib`（ai::external）通过（25 passed）

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-13 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-13 | ⬜ | 分析报告事实核查批次录入 |
| 2026-09-13 | ✅ | 复核证据成立。修复：① `try_serve` 生成 per-boot 32 字节 OsRng token（`crypto::secret::random_secret`），写入 `ExternalEndpointInfo.token`（`#[serde(default)]` 兼容旧 discovery 读取）并注入连接处理；② `parse_head` 完整收集请求头（小写键），新增 `validate_request`：`Authorization: Bearer <token>`（常量时间比较，401）→ `Content-Type` 必须 `application/json`（415，阻断浏览器 `text/plain` 简单请求）→ Host 仅本机（403，防 DNS rebinding）→ Origin 存在时仅接受本机源（403，恶意网页）；③ CLI `cmd_call` 从 discovery 读 token 附 `Authorization` 头，401 时提示重启应用（discovery 可能属旧实例）；405 方法检查先于鉴权执行。边界说明：同机同用户进程仍可读 discovery 文件拿 token——威胁模型以浏览器 CSRF 与跨用户进程为主，与任务文档修复建议一致。验证：`cargo test --lib ai::external` 25 passed（新增 4 用例：鉴权拒绝矩阵 / token 生成 / 常量时间比较 / 请求头收集）。 |
