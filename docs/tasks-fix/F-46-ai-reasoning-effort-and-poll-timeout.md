# F-46 AI 生成 Commit Message 耗时长（思考型模型默认开启思考，无关闭入口）+ 生成轮询 30s 超时

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-20 用户反馈：AI 生成 Commit Message「生成的时间好长，是不是默认开启的思考，ai commit message 这种的应该不用思考吧」 |
| 关联任务 | F-48（思考增量不透传）、F-47（commit message 结果丢失） |

## 问题描述

1. 用户配置的模型是思考型（reasoning）模型，Provider 侧默认开启思考；Commit
   Message 这类轻任务本不需要长思考，但应用**没有任何关闭/调低思考的入口**——
   `ProviderRequest`（`ai/adapters/mod.rs:61`）根本没有 reasoning 相关字段，
   三个 adapter 组装请求体时也不带任何思考控制参数。
2. 前端轮询结果有硬上限：`pollAiResult`（`src/views/RepositoryList.vue:1827`）
   60×500ms = **30 秒**后抛「AI 生成超时」。思考型模型随便就超过 30s，即使
   后端最终会成功，前端也已报错放弃。

## 根因（已定位）

- 思考参数缺失：`build_body`（`ai/adapters/openai_chat.rs:70` /
  `openai_responses.rs` / `anthropic.rs`）不携带任何 reasoning/thinking 参数，
  一切由 Provider 模型默认值决定。
- 各协议方言不同：OpenAI 系 `reasoning_effort` / Responses `reasoning.effort`；
  通义 Qwen3 `enable_thinking: false`；火山 Doubao / 智谱 GLM
  `thinking: {"type":"disabled"}`；Anthropic 不传 `thinking` 即不思考。
- 30s 轮询上限是任意值，与后端实际的 120s 请求超时（`gateway.rs` 默认配置）
  不匹配。

## 修复范围

- [x] 模型默认值新增可选字段 `reasoningEffort`（`AiModelDefaults`：
      `off` / `low` / `medium` / `high`，缺省 = 不传任何参数，保持现状零回归）
- [x] adapter 按方言映射：openai_chat → `off` 发 `enable_thinking:false` +
      `thinking:{type:disabled}` + `reasoning_effort:minimal`（覆盖国产模型
      方言与 OpenAI 系），其余档发 `reasoning_effort`；openai_responses →
      `reasoning.effort`（`off` 映射 `minimal`）；anthropic → `low/medium/high`
      发 `thinking:{type:enabled,budget_tokens:N}`（1024/4096/16384，钳制
      < max_tokens，不足 1024 降级不传；thinking 与自定义 temperature 不兼容，
      开启时移除 temperature），`off` = 不传，本就如此
- [x] 模型设置 UI（`AiModelsSection.vue`）加「思考程度」下拉，说明文案注明
      「Provider 不识别的参数可能被拒（400），此时请改回默认」
- [x] `pollAiResult` 轮询上限从 30s 放宽到与后端请求超时对齐（240×500ms=120s），
      超时文案引导用户「可在模型设置中关闭/调低思考」

## 验收标准

- [x] `GW_TEST_MANIFEST=1 cargo test --lib -- ai::` 全绿（含新增 adapter
      请求体断言：off 三方言齐发 / 其余档单发 / Responses effort 映射 /
      Anthropic budget 钳制与 temperature 移除，共 5 例）
- [x] 前端 `vue-tsc` 通过
- [x] 真机：把当前模型设为「关闭思考」后重新生成 Commit Message，耗时明显下降
      （代码+单测+类型检查全绿；真机耗时对比待用户实测）

## 进度

### 状态

- 当前状态：✅ 已完成（代码 + 单测 + 类型检查全绿；真机耗时对比待用户实测）
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：`ProviderRequest` 无 reasoning 字段，adapter 不带思考控制参数；前端轮询 30s 硬上限 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：`AiModelDefaults.reasoningEffort`（off/low/medium/high，缺省不传零回归）经 Gateway 透传至 `ProviderRequest`，三 adapter 按方言映射；`AiModelsSection` 加「思考程度」下拉；`pollAiResult` 30s→120s 且超时文案引导调低思考；`cargo test --lib -- ai::` 全绿，vue-tsc 通过 |
