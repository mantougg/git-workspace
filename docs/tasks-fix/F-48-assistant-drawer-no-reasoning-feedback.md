# F-48 Assistant 对话框：发送后长时间无任何反馈（思考增量不透传），发送按钮一直禁用

| 项 | 值 |
|---|---|
| 优先级 | P1 |
| 状态 | ✅ 已完成 |
| 来源 | 2026-09-20 用户反馈：「侧边栏的 Assistant 对话框，不会展示思考内容，发送之后没有反应，但是发送按钮变成禁用的了」 |
| 关联任务 | F-46（思考参数控制） |

## 问题描述

Assistant 抽屉（`AssistantDrawer.vue`）发送消息后：界面没有任何输出、发送按钮
进入禁用态不再恢复。用户无法区分「模型在思考」「请求卡死」「请求已失败」。

## 根因（已定位）

1. **思考增量被全链路丢弃**：openai_chat adapter 只映射 `delta.content`
   （`ai/adapters/openai_chat.rs:146`），`reasoning_content` 增量被丢；
   anthropic 显式 `Skip` 掉 `thinking_delta`（`anthropic.rs:163`）；
   openai_responses 的 reasoning item 同样不进文本流。思考型模型的思考阶段
   （可达数分钟）前端一个字符都收不到。
2. **空闲超时被思考流量喂活**：SSE 泵的块间空闲超时直接复用整请求超时 120s
   （`openai_chat.rs:59-64` 把 `ctx.timeout` 传给 `spawn_sse_pump`），思考增量
   虽被丢弃但字节持续到达、不断重置空闲计时——既不超时也无输出，请求长期挂在
   `Streaming` 阶段，前端 `sending`（`stores/ai.ts:262`）恒为 true → 按钮禁用。
3. **失败后状态可恢复**（`finalize` 会写 `lastError`），所以「永久无反应」的
   现象指向「思考中但无反馈」，而非崩死。

## 修复范围

- [x] 流式管道新增思考增量类型：`AiStreamChunk::ReasoningDelta { text }`
      （同步 IPC golden：`models/ipc_golden` 样本 + `golden/ipc_samples.json`
      已重新生成）；gateway `run_stream` 透传（不计入正文 text/输出字符）
- [x] adapter 映射：openai_chat `delta.reasoning_content` → ReasoningDelta；
      anthropic `thinking_delta` → ReasoningDelta；openai_responses
      `response.reasoning_summary_text.delta` / `response.reasoning_text.delta`
      → ReasoningDelta
- [x] 前端：`stores/ai.ts` 收集 `streamingReasoning`；`ConversationView.vue`
      渲染为可折叠的「思考过程」弱化块（默认折叠，不占正文）；
      终态（finalize）后丢弃不持久化
- [x] 泵任务空闲超时与整请求超时解耦：新增独立
      `GatewayConfig.stream_idle_timeout` / `AdapterContext.stream_idle_timeout`
      （默认 120s 与 request_timeout 相同），注释说明语义差异——它判定
      「连接疑似死亡」而非「请求总时长」，思考字节流量会重置它（长思考不
      被判死），需要更短上限时独立调小、不影响非流式整请求超时

## 验收标准

- [x] 思考型模型发送后，drawer 内实时看到「思考中…」内容滚动；正式回答随后
      到达（代码链路：adapter→泵→gateway 事件→store→折叠块；真机待用户实测）
- [x] `GW_TEST_MANIFEST=1 cargo test --lib -- ai::` 全绿（含新 chunk 的单测：
      三 adapter 映射各 1 例 + gateway 透传不计正文 1 例）
- [x] `vue-tsc` 通过

## 进度

### 状态

- 当前状态：✅ 已完成（代码 + 单测 + golden + 类型检查全绿；真机思考展示待用户实测）
- 最近更新：2026-09-20 修复完成

### 时间线

| 日期 | 状态 | 说明 |
|---|---|---|
| 2026-09-20 | ⬜ | 问题录入；定位：思考增量全链路丢弃 + 泵空闲超时被思考流量喂活 → 长思考期间零反馈、按钮恒禁用 |
| 2026-09-20 | 🟦 | 开始修复 |
| 2026-09-20 | ✅ | 修复完成：`StreamItem::Reasoning` / `SseAction::EmitReasoning` / `AiStreamChunk::ReasoningDelta` 全链路透传（不计正文与输出字符，终态丢弃）；三协议 adapter 映射 reasoning_content/thinking_delta/reasoning summary delta；`stream_idle_timeout` 与整请求超时解耦；前端 `streamingReasoning` + 可折叠「思考过程」弱化块；新增 4 例单测 + golden 同步，ai:: 与 ipc_golden 全绿，vue-tsc 通过 |
