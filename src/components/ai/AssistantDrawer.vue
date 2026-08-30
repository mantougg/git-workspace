<template>
  <n-drawer v-model:show="ai.drawerOpen" :width="460" placement="right">
    <n-drawer-content closable body-content-style="padding: 0; display: flex; flex-direction: column;">
      <template #header>
        <div class="drawer-title">
          <span>GitWorkspace Assistant</span>
          <n-tag size="small" :bordered="false">{{ roleLabels[ai.role] }}</n-tag>
        </div>
      </template>

      <div class="drawer-body">
        <!-- 顶部：角色 / 模型 / 上下文范围（§12.3） -->
        <div class="drawer-top">
          <div class="role-row">
            <n-select
              size="small"
              :value="ai.role"
              :options="roleOptions"
              class="role-select"
              @update:value="(v: AiSessionRole) => ai.setRoleManual(v)"
            />
            <n-tag v-if="!ai.roleIsManual" size="small" type="info" :bordered="false">
              自动推断
            </n-tag>
          </div>
          <div class="scope-row" :title="scopeLabel">
            <n-icon :size="12"><LocateOutline /></n-icon>
            <span class="scope-text">{{ scopeLabel }}</span>
          </div>
          <div v-if="modelLabel" class="model-row">模型：{{ modelLabel }}</div>
        </div>

        <!-- 会话管理条 -->
        <div class="session-bar">
          <n-popover trigger="click" placement="bottom-start" :style="{ maxHeight: '320px', overflowY: 'auto' }">
            <template #trigger>
              <n-button size="small" quaternary :loading="ai.sessionsLoading">
                {{ ai.currentSession?.title ?? "新会话" }}
                <n-icon :size="12"><ChevronDownOutline /></n-icon>
              </n-button>
            </template>
            <div class="session-list">
              <div
                v-for="session in ai.sessions"
                :key="session.id"
                class="session-item"
                :class="{ active: session.id === ai.currentSession?.id }"
                @click="ai.selectSession(session.id)"
              >
                <span class="session-title">{{ session.title }}</span>
                <span class="session-meta">
                  {{ roleLabels[session.role] }} · {{ session.messageCount }} 条
                </span>
              </div>
              <n-empty v-if="ai.sessions.length === 0" size="small" description="暂无历史会话" />
              <n-button
                v-if="ai.sessionsHasMore"
                size="tiny"
                block
                :loading="ai.sessionsLoading"
                @click="ai.loadMoreSessions()"
              >
                加载更多会话
              </n-button>
            </div>
          </n-popover>

          <n-button size="small" quaternary @click="ai.newSession()">
            <template #icon><n-icon :size="14"><AddOutline /></n-icon></template>
            新会话
          </n-button>

          <n-dropdown
            trigger="click"
            :options="sessionMenuOptions"
            @select="onSessionMenu"
          >
            <n-button size="small" quaternary>
              <n-icon :size="14"><EllipsisHorizontalOutline /></n-icon>
            </n-button>
          </n-dropdown>
        </div>

        <!-- 降级：未配置（§12.4） -->
        <n-alert v-if="ai.settingsLoaded && !ai.configured" type="warning" :show-icon="false" class="drawer-alert">
          <div class="error-line">AI 未配置，助手暂时无法联网回答。本地 Git/Runtime 功能不受影响。</div>
          <div class="error-actions">
            <n-button size="tiny" @click="goAiSettings">配置 AI</n-button>
          </div>
        </n-alert>

        <!-- 降级：请求失败（§12.4：保留输入与上下文，可重试/缩小范围） -->
        <n-alert v-if="ai.lastError" type="error" :show-icon="false" class="drawer-alert">
          <div class="error-line">{{ ai.lastError.message }}</div>
          <div class="error-actions">
            <n-button size="tiny" :disabled="!ai.preview" @click="ai.retry()">重试</n-button>
            <n-button size="tiny" @click="ai.clearContext()">缩小范围</n-button>
            <n-button size="tiny" @click="goAiSettings">配置 AI</n-button>
          </div>
        </n-alert>

        <!-- 带入上下文（可清空） -->
        <div v-if="ai.scope.supplementary.length > 0" class="context-chips">
          <n-tag
            v-for="item in ai.scope.supplementary"
            :key="item.sourceId"
            size="small"
            :bordered="false"
          >
            {{ item.displayName }}
          </n-tag>
          <n-button size="tiny" quaternary @click="ai.clearContext()">清空上下文</n-button>
        </div>

        <!-- 中部：会话消息 / 工具读取摘要 -->
        <ConversationView
          :messages="ai.detail?.messages ?? []"
          :can-load-earlier="ai.messagesHasMore"
          :streaming-text="ai.streamingText"
          :tool-reads="ai.toolReads"
          @load-earlier="ai.loadEarlierMessages()"
        />

        <!-- 只读工具（§9.3/§9.4：白名单 + 单次上限 8，不自动操作） -->
        <div v-if="runnableTools.length > 0" class="tool-bar">
          <div class="tool-bar-header">
            <span>读取应用状态</span>
            <span class="tool-limit">本轮还可调用 {{ ai.toolCallLimit - ai.toolCallCount }} 次</span>
          </div>
          <div class="tool-buttons">
            <n-button
              v-for="tool in runnableTools"
              :key="tool.name"
              size="tiny"
              tertiary
              :loading="ai.toolRunning === tool.name"
              :disabled="ai.toolRunning != null || ai.toolCallCount >= ai.toolCallLimit"
              @click="ai.runTool(tool)"
            >
              {{ tool.name }}
            </n-button>
          </div>
        </div>

        <!-- 底部：输入 / 发送 / 取消 / 清空上下文（§12.3） -->
        <div class="drawer-footer">
          <n-input
            v-model:value="ai.input"
            type="textarea"
            :autosize="{ minRows: 2, maxRows: 6 }"
            placeholder="提问当前工作区 / 仓库 / Runtime 状态…（Enter 发送，Shift+Enter 换行）"
            :disabled="!ai.configured && ai.settingsLoaded"
            @keydown.enter.exact.prevent="ai.send()"
          />
          <div class="footer-actions">
            <n-button size="small" quaternary @click="ai.clearContext()">清空上下文</n-button>
            <n-button v-if="ai.sending" size="small" @click="ai.cancel()">取消</n-button>
            <n-button
              size="small"
              type="primary"
              :loading="ai.building"
              :disabled="!ai.input.trim() || ai.sending || (!ai.configured && ai.settingsLoaded)"
              @click="ai.send()"
            >
              发送
            </n-button>
          </div>
        </div>
      </div>
    </n-drawer-content>
  </n-drawer>

  <!-- 发送前 Preview（硬要求，§10.1） -->
  <AiRequestPreview
    v-model="ai.previewVisible"
    :preview="ai.preview"
    :loading="ai.building"
    :confirming="ai.confirming"
    @confirm="ai.confirmSend()"
    @toggle-exclusion="ai.toggleExclusion"
    @confirm-warn="ai.confirmWarn()"
  />

  <!-- 重命名会话 -->
  <n-modal v-model:show="renameVisible" preset="card" title="重命名会话" style="width: 360px">
    <n-input v-model:value="renameText" placeholder="会话标题" @keydown.enter="confirmRename" />
    <template #footer>
      <n-button @click="renameVisible = false">取消</n-button>
      <n-button type="primary" :disabled="!renameText.trim()" @click="confirmRename">确定</n-button>
    </template>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, h, ref } from "vue";
import { useRouter } from "vue-router";
import { useDialog, useMessage, NIcon, NSwitch } from "naive-ui";
import { save } from "@tauri-apps/plugin-dialog";
import {
  AddOutline,
  ChevronDownOutline,
  EllipsisHorizontalOutline,
  LocateOutline,
} from "@vicons/ionicons5";
import AiRequestPreview from "@/components/ai/AiRequestPreview.vue";
import ConversationView from "@/components/ai/ConversationView.vue";
import { useAiAssistant, ROLE_LABELS } from "@/composables/useAiAssistant";
import { buildToolArguments } from "@/stores/ai";
import { errMsg } from "@/utils/error";
import type { AiSessionRole } from "@/types/ai";

const { ai, scopeLabel, roleLabels } = useAiAssistant();
const router = useRouter();
const dialog = useDialog();
const message = useMessage();

const roleOptions = (Object.keys(ROLE_LABELS) as AiSessionRole[]).map((value) => ({
  value,
  label: ROLE_LABELS[value],
}));

/** 必需参数可全部由当前作用域满足的工具才可一键运行。 */
const runnableTools = computed(() =>
  ai.availableTools.filter((tool) => buildToolArguments(tool, ai.scope) != null),
);

const modelLabel = computed(() => {
  if (ai.preview) return `${ai.preview.providerName} · ${ai.preview.modelName}`;
  const chatDefault = ai.settingsSummary?.taskDefaults.find((t) => t.taskKind === "chat");
  return chatDefault ? chatDefault.modelId : null;
});

// -- 会话菜单（重命名 / 导出 / 删除 / 持久化开关） -------------------------------

const renameVisible = ref(false);
const renameText = ref("");

const sessionMenuOptions = computed(() => [
  { label: "重命名会话", key: "rename", disabled: !ai.currentSession },
  { label: "导出会话（Markdown）", key: "export", disabled: !ai.currentSession },
  { label: "删除会话", key: "delete", disabled: !ai.currentSession },
  { type: "divider", key: "divider" },
  {
    label: () =>
      h("div", { style: "display: flex; align-items: center; gap: 8px;" }, [
        h("span", "保存会话记录"),
        h(NSwitch, {
          size: "small",
          value: ai.persistence?.persistSessions ?? false,
          "onUpdate:value": () => ai.togglePersistence(),
          onClick: (e: MouseEvent) => e.stopPropagation(),
        }),
      ]),
    key: "persistence",
  },
]);

function onSessionMenu(key: string) {
  if (key === "rename") {
    renameText.value = ai.currentSession?.title ?? "";
    renameVisible.value = true;
  } else if (key === "export") {
    void exportSession();
  } else if (key === "delete") {
    const id = ai.currentSession?.id;
    if (!id) return;
    dialog.warning({
      title: "删除会话",
      content: "将级联删除该会话的全部消息与相关本地缓存，不可恢复。",
      positiveText: "删除",
      negativeText: "取消",
      onPositiveClick: async () => {
        try {
          await ai.removeSession(id);
          message.success("会话已删除");
        } catch (error) {
          message.error("删除失败：" + errMsg(error));
        }
      },
    });
  }
}

async function confirmRename() {
  try {
    await ai.renameCurrent(renameText.value);
    renameVisible.value = false;
    message.success("会话已重命名");
  } catch (error) {
    message.error("重命名失败：" + errMsg(error));
  }
}

async function exportSession() {
  const title = ai.currentSession?.title ?? "会话";
  const dest = await save({
    title: "导出会话",
    defaultPath: `${title}.md`,
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (typeof dest !== "string") return;
  try {
    const outcome = await ai.exportCurrent(dest);
    if (outcome) {
      message.success(`已导出 ${outcome.messageCount} 条消息 → ${outcome.path}`);
    }
  } catch (error) {
    message.error("导出失败：" + errMsg(error));
  }
}

function goAiSettings() {
  router.push({ name: "ai-settings" });
}
</script>

<style scoped>
.drawer-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.drawer-body {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.drawer-top {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  padding: var(--gw-space-2) var(--gw-space-3);
  border-bottom: 1px solid var(--gw-border);
}

.role-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.role-select {
  flex: 1;
}

.scope-row {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
  min-width: 0;
}

.scope-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.model-row {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.session-bar {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  padding: var(--gw-space-1) var(--gw-space-3);
  border-bottom: 1px solid var(--gw-border);
}

.session-list {
  min-width: 240px;
  display: flex;
  flex-direction: column;
}

.session-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--gw-space-1) var(--gw-space-2);
  border-radius: var(--gw-radius-sm);
  cursor: pointer;
}

.session-item:hover {
  background: var(--gw-bg-hover);
}

.session-item.active .session-title {
  color: var(--gw-accent);
}

.session-title {
  font-size: var(--gw-text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.session-meta {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.drawer-alert {
  margin: var(--gw-space-2) var(--gw-space-3) 0;
}

.error-line {
  margin-bottom: var(--gw-space-1);
}

.error-actions {
  display: flex;
  gap: var(--gw-space-1);
}

.context-chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--gw-space-1);
  padding: var(--gw-space-2) var(--gw-space-3) 0;
}

.tool-bar {
  border-top: 1px solid var(--gw-border);
  padding: var(--gw-space-2) var(--gw-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.tool-bar-header {
  display: flex;
  justify-content: space-between;
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.tool-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: var(--gw-space-1);
}

.drawer-footer {
  border-top: 1px solid var(--gw-border);
  padding: var(--gw-space-2) var(--gw-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
}

.footer-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--gw-space-1);
}
</style>
