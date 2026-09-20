# F-49 AI 402 余额不足错误提示语义化（当前笼统显示「Provider 拒绝了请求」）

| 项 | 值 |
|---|---|
| 优先级 | P2 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-20 用户反馈：AI 生成 Commit Message 报「Provider 拒绝了请求: Provider 返回 HTTP 402（insufficient_credits）」，提示没有指出真正原因（额度不足），用户难以自行定位 |
| 关联任务 | 无 |

## 问题描述

Provider 返回 HTTP 402（余额/额度不足，中转站常见）时，错误被归入兜底分支
`AiPolicyRejected`（`ai/adapters/mod.rs:263`），用户看到的提示是
「Provider 拒绝了请求」，既不说是额度问题，建议行动（`suggested_actions`）
也只有「调整请求内容」「检查内容策略」这类不对症的方向。

本案例中用户实际配置了**两个 Provider**，任务默认链解析到了已失效的第一个
（`gateway.rs:262` `resolve_model`），删掉它之后即恢复——402 语义化提示能
让用户第一时间定位到「哪个 Provider 没钱了」。

## 根因（已定位）

`classify_status`（`ai/adapters/mod.rs:222`）只特判 401/403/429/404/5xx，
402 落兜底 `PolicyRejected`。`AiError` 没有额度类变体，错误详情里也不带
「是哪个 Provider/模型」的上下文。

## 修复范围

- [x] `AiError` 新增 `QuotaExceeded { message }` 变体（code
      `AiQuotaExceeded`），suggested actions：「为该 Provider 账户充值或提升
      额度」「在 AI 设置中检查任务默认链指向的 Provider」「更换有额度的
      Provider/Key」
- [x] `classify_status` 增加 402 分支 → `QuotaExceeded`，message 形如
      「Provider 返回 HTTP 402（余额/额度不足）：请充值或更换有额度的 Key」
- [x] `details_json` 对 `QuotaExceeded` 带上 providerId/modelId（请求上下文中
      已有，非敏感），帮助用户定位默认链解析到了谁
- [x] 单测：402 → AiQuotaExceeded；错误 code 表回归（error.rs 全变体测试）

## 验收标准

- [x] 402 响应的用户可见提示明确包含「余额/额度不足」与可行动建议
- [x] `GW_TEST_MANIFEST=1 cargo test --lib -- ai::` 全绿

## 进度

### 状态

- 当前状态：✅ 已完成
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`classify_status` 无 402 分支，落兜底 `PolicyRejected` 语义失真 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：`AiError::QuotaExceeded`（code `AiQuotaExceeded`，details 带 providerId/modelId，建议行动指向充值/检查默认链/换 Key）+ `classify_status` 402 分支（附 `insufficient_credits` 等错误码标签）；新增 2 例单测，`cargo test --lib -- ai::` 全绿 |
