<template>
  <div class="section">
    <n-alert type="info" :show-icon="false">
      本区块展示 AI 隐私与安全的硬约束。发送前 Preview 是硬要求，<b>不提供关闭开关</b>。
    </n-alert>

    <n-descriptions label-placement="left" bordered :column="1" size="small">
      <n-descriptions-item label="发送前 Preview">
        <div class="desc-cell">
          <n-tag type="success" size="small" :bordered="false">强制开启</n-tag>
          <span>发送内容、范围、token 估算、Secret 结果与脱敏项须用户确认后才联网。统一 Preview 流程随 AI-03 落地；当前版本原型 ai_review 保留发送前 Secret 阻断扫描。</span>
        </div>
      </n-descriptions-item>
      <n-descriptions-item label="Secret 策略">
        <div class="desc-cell">
          <n-tag size="small" :bordered="false">复用 T-08</n-tag>
          <span>Block（默认阻断）/ Mask / Exclude / Warn（低置信度需确认）。检测发生在最终内容生成之后，脱敏后二次扫描。</span>
        </div>
      </n-descriptions-item>
      <n-descriptions-item label="凭证安全">
        <div class="desc-cell">
          <n-tag size="small" :bordered="false">仅 OS 凭证存储</n-tag>
          <span>API Key 只存 Windows Credential Manager / macOS Keychain / Linux Secret Service；存储不可用时仅本次会话内存保存，不回退普通文件。Key 不进日志、错误信息、LocalStorage 与 URL。</span>
        </div>
      </n-descriptions-item>
      <n-descriptions-item label="上下文最小化">
        <div class="desc-cell">
          <n-tag size="small" :bordered="false">Context First</n-tag>
          <span>只发送任务所需的结构化摘要与片段，不默认发送整个 Repository；上下文带 manifest（来源/范围/token/脱敏标记）随 AI-03 落地。</span>
        </div>
      </n-descriptions-item>
      <n-descriptions-item label="会话与审计">
        <div class="desc-cell">
          <n-tag type="info" size="small" :bordered="false">AI-04</n-tag>
          <span>会话/消息/请求审计与「不保存完整 Prompt 原文」的数据保留策略随 AI-04 落地。</span>
        </div>
      </n-descriptions-item>
      <n-descriptions-item label="离线可用">
        <div class="desc-cell">
          <n-tag type="success" size="small" :bordered="false">Offline First</n-tag>
          <span>AI 不可达不影响 Git/Runtime 核心功能；网络不可用时保留上下文允许重试，不假装使用了本地模型。</span>
        </div>
      </n-descriptions-item>
    </n-descriptions>
  </div>
</template>

<style scoped>
.section {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
}

.desc-cell {
  display: flex;
  gap: var(--gw-space-2);
  align-items: flex-start;
}

.desc-cell > n-tag {
  flex-shrink: 0;
}
</style>
