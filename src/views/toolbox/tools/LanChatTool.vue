<template>
  <div class="lan-chat-tool">
    <!-- ═══ 未进房：创建 / 加入 ═══ -->
    <template v-if="!room">
      <n-tabs v-model:value="entryTab" type="segment" class="entry-tabs">
        <n-tab-pane name="create" tab="创建房间">
          <div class="entry-form">
            <div class="field">
              <div class="field-label">房间名</div>
              <n-input
                v-model:value="createForm.roomName"
                placeholder="如：Backend Team"
                :maxlength="32"
              />
            </div>
            <div class="field">
              <div class="field-label">加密 Secret（Shared Secret）</div>
              <div class="secret-row">
                <n-input
                  v-model:value="createForm.secret"
                  type="password"
                  show-password-on="click"
                  placeholder="共享给同事的加密密钥，不是登录密码"
                />
                <n-button :loading="generating" @click="onGenerateSecret">
                  生成
                </n-button>
              </div>
              <div class="hint">
                同一房间的成员必须使用相同的 Secret 才能解密消息。
              </div>
            </div>
            <div class="field">
              <div class="field-label">昵称</div>
              <n-input
                v-model:value="createForm.nickname"
                placeholder="你在房间里的名字"
                :maxlength="24"
              />
            </div>
            <div>
              <n-button
                type="primary"
                :loading="busy"
                :disabled="!canCreate"
                @click="onCreateRoom"
              >
                创建房间
              </n-button>
            </div>
          </div>
        </n-tab-pane>

        <n-tab-pane name="join" tab="加入房间">
          <div class="entry-form">
            <div class="field">
              <div class="field-label">附近房间（自动发现）</div>
              <div v-if="nearbyRooms.length > 0" class="nearby-list">
                <div
                  v-for="r in nearbyRooms"
                  :key="r.roomId"
                  class="nearby-item"
                  :class="{ active: joinForm.roomId === r.roomId }"
                  @click="onSelectNearby(r)"
                >
                  <span class="nearby-name">{{ r.roomName }}</span>
                  <span class="mono nearby-addr">{{ r.addr }}:{{ r.port }}</span>
                </div>
              </div>
              <div v-else class="hint">
                暂未发现附近房间，可在下方手动填写房间 ID 与 Bootstrap 地址（跨子网时）。
              </div>
            </div>
            <div class="field">
              <div class="field-label">房间 ID</div>
              <n-input
                v-model:value="joinForm.roomId"
                class="mono-input"
                placeholder="从创建者处获取的房间 ID"
              />
            </div>
            <div class="field">
              <div class="field-label">Bootstrap 地址（IP:Port，可选）</div>
              <div class="bootstrap-row">
                <n-input
                  v-model:value="joinForm.bootstrap"
                  class="mono-input"
                  placeholder="同子网可留空（自动发现）；跨子网填房间任一员地址，如 192.168.1.5:45678"
                />
                <n-button
                  :disabled="nearbyRooms.length === 0"
                  @click="onAutoFillBootstrap"
                >
                  自动填入
                </n-button>
              </div>
            </div>
            <div class="field">
              <div class="field-label">加密 Secret（Shared Secret）</div>
              <n-input
                v-model:value="joinForm.secret"
                type="password"
                show-password-on="click"
                placeholder="房间创建者共享的加密密钥"
              />
            </div>
            <div class="field">
              <div class="field-label">昵称</div>
              <n-input
                v-model:value="joinForm.nickname"
                placeholder="你在房间里的名字"
                :maxlength="24"
              />
            </div>
            <div>
              <n-button
                type="primary"
                :loading="busy"
                :disabled="!canJoin"
                @click="onJoinRoom"
              >
                加入房间
              </n-button>
            </div>
          </div>
        </n-tab-pane>
      </n-tabs>
    </template>

    <!-- ═══ 已进房：聊天室 ═══ -->
    <template v-else>
      <div class="room-header">
        <div class="room-title">
          <span class="room-name">{{ room.roomName }}</span>
          <n-popover trigger="click" placement="bottom-start">
            <template #trigger>
              <n-tag size="small" type="success" class="secure-tag">
                🔒 端到端加密
              </n-tag>
            </template>
            <n-descriptions :column="1" label-placement="left" size="small">
              <n-descriptions-item label="加密">
                XChaCha20-Poly1305
              </n-descriptions-item>
              <n-descriptions-item label="密钥派生">
                Argon2id
              </n-descriptions-item>
              <n-descriptions-item label="密钥交换">
                预共享（Shared Secret）
              </n-descriptions-item>
              <n-descriptions-item label="消息持久化">
                无（仅存内存，离开即销毁）
              </n-descriptions-item>
              <n-descriptions-item label="前向保密">
                不支持
              </n-descriptions-item>
            </n-descriptions>
          </n-popover>
        </div>
        <div class="room-meta">
          <span class="hint">● {{ room.members.length }} 成员 · {{ room.connectedPeers }} 连接 · P2P Mesh</span>
          <span class="hint mono">ID: {{ room.roomId }}</span>
          <n-button size="tiny" tertiary @click="copyText(room.roomId, '房间 ID')">
            复制 ID
          </n-button>
          <n-popover trigger="click" placement="bottom-end">
            <template #trigger>
              <n-button size="tiny" tertiary>本机地址</n-button>
            </template>
            <div class="share-panel">
              <div v-for="addr in listenAddrs" :key="addr" class="share-row">
                <span class="mono share-value">{{ addr }}</span>
                <n-button
                  size="tiny"
                  tertiary
                  @click="copyText(addr, 'Bootstrap 地址')"
                >
                  复制
                </n-button>
              </div>
              <div v-if="listenAddrs.length === 0" class="hint">
                未获取到本机局域网 IP，请手动告知：本机IP:{{ room.port }}
              </div>
              <div class="hint">
                跨子网成员加入时需要：房间 ID + 任一 Bootstrap 地址 + 相同 Secret。
              </div>
            </div>
          </n-popover>
          <n-button
            size="small"
            type="error"
            secondary
            :loading="busy"
            @click="onLeaveRoom"
          >
            离开房间
          </n-button>
        </div>
      </div>

      <div class="room-body">
        <div class="chat-area">
          <div ref="msgListEl" class="msg-list">
            <n-empty
              v-if="messages.length === 0"
              description="还没有消息，打个招呼吧"
              class="msg-empty"
            />
            <div
              v-for="m in messages"
              :key="m.messageId"
              class="msg-row"
              :class="{ mine: m.mine }"
            >
              <div class="msg-bubble">
                <div class="msg-meta">
                  <span class="msg-sender">{{ m.mine ? "我" : m.senderName }}</span>
                  <span class="msg-time">{{ formatTime(m.timestamp) }}</span>
                </div>
                <div class="msg-content">{{ m.content }}</div>
              </div>
            </div>
          </div>
          <div class="input-row">
            <n-input
              v-model:value="draft"
              type="textarea"
              :autosize="{ minRows: 1, maxRows: 4 }"
              placeholder="输入消息，Enter 发送，Shift+Enter 换行"
              @keydown.enter.exact.prevent="onSend"
            />
            <n-button
              type="primary"
              :disabled="!draft.trim()"
              :loading="sending"
              @click="onSend"
            >
              发送
            </n-button>
          </div>
        </div>

        <div class="member-list">
          <div class="member-header">成员（{{ room.members.length }}）</div>
          <div
            v-for="mem in room.members"
            :key="mem.peerId"
            class="member-item"
          >
            <span class="member-dot" />
            <span class="member-name">{{ mem.nickname }}</span>
            <span v-if="mem.isSelf" class="member-self">（自己）</span>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref } from "vue";
import {
  NButton,
  NDescriptions,
  NDescriptionsItem,
  NEmpty,
  NInput,
  NPopover,
  NTabPane,
  NTabs,
  NTag,
  useMessage,
} from "naive-ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  lanChatCreateRoom,
  lanChatGenerateSecret,
  lanChatJoinRoom,
  lanChatLeaveRoom,
  lanChatLocalAddrs,
  lanChatRoomState,
  lanChatSendMessage,
  lanChatStartDiscovery,
  lanChatStopDiscovery,
  type LanChatError,
  type LanChatMessage,
  type LanChatNearbyRoom,
  type LanChatRoomState,
} from "@/api/lanChat";
import { errMsg } from "@/utils/error";

const message = useMessage();

// ── 房间状态（事件推全量快照；聊天室不留历史，消息只存内存）──
const room = ref<LanChatRoomState | null>(null);
const messages = ref<LanChatMessage[]>([]);
const seenMessageIds = new Set<string>();
const nearbyRooms = ref<LanChatNearbyRoom[]>([]);
// 本机局域网 IPv4 列表（进房后拉取一次，用于拼「IP:Port」分享地址）。
const localAddrs = ref<string[]>([]);

const entryTab = ref<"create" | "join">("create");
const createForm = reactive({ roomName: "", secret: "", nickname: "" });
const joinForm = reactive({ roomId: "", secret: "", nickname: "", bootstrap: "" });

const busy = ref(false);
const sending = ref(false);
const generating = ref(false);
const draft = ref("");
const msgListEl = ref<HTMLElement | null>(null);

const unlisteners: UnlistenFn[] = [];

const canCreate = computed(
  () =>
    createForm.roomName.trim() !== "" &&
    createForm.secret.trim() !== "" &&
    createForm.nickname.trim() !== "",
);
const canJoin = computed(
  () =>
    joinForm.roomId.trim() !== "" &&
    joinForm.secret.trim() !== "" &&
    joinForm.nickname.trim() !== "",
);
// 可分享的 Bootstrap 地址：本机每个局域网 IP × 房间监听端口。
const listenAddrs = computed(() =>
  room.value ? localAddrs.value.map((ip) => `${ip}:${room.value!.port}`) : [],
);

function applyRoomState(state: LanChatRoomState | null) {
  room.value = state;
  if (state) {
    void refreshLocalAddrs();
  } else {
    messages.value = [];
  }
}

async function refreshLocalAddrs() {
  try {
    localAddrs.value = await lanChatLocalAddrs();
  } catch {
    localAddrs.value = [];
  }
}

async function copyText(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success(`${label}已复制`);
  } catch {
    message.error("复制失败，请手动选择复制");
  }
}

async function onGenerateSecret() {
  generating.value = true;
  try {
    const s = await lanChatGenerateSecret();
    createForm.secret = s.base64Url;
  } catch (e) {
    message.error("生成 Secret 失败：" + errMsg(e));
  } finally {
    generating.value = false;
  }
}

async function onCreateRoom() {
  if (!canCreate.value) return;
  busy.value = true;
  try {
    applyRoomState(
      await lanChatCreateRoom({
        roomName: createForm.roomName.trim(),
        secret: createForm.secret,
        nickname: createForm.nickname.trim(),
      }),
    );
    await stopDiscoveryQuietly();
  } catch (e) {
    message.error("创建房间失败：" + errMsg(e));
  } finally {
    busy.value = false;
  }
}

function onSelectNearby(r: LanChatNearbyRoom) {
  joinForm.roomId = r.roomId;
  joinForm.bootstrap = `${r.addr}:${r.port}`;
}

/** 「自动填入」：优先按已输入的房间 ID 匹配附近房间，否则取发现的第一个。 */
function onAutoFillBootstrap() {
  const typedId = joinForm.roomId.trim();
  const match = typedId
    ? nearbyRooms.value.find((r) => r.roomId === typedId)
    : nearbyRooms.value[0];
  if (!match) {
    message.warning(
      typedId
        ? "附近未发现该房间 ID，请确认与创建者在同一子网"
        : "暂未发现附近房间",
    );
    return;
  }
  joinForm.roomId = match.roomId;
  joinForm.bootstrap = `${match.addr}:${match.port}`;
  message.success(`已填入「${match.roomName}」的地址`);
}

async function onJoinRoom() {
  if (!canJoin.value) return;
  busy.value = true;
  try {
    applyRoomState(
      await lanChatJoinRoom({
        roomId: joinForm.roomId.trim(),
        secret: joinForm.secret,
        nickname: joinForm.nickname.trim(),
        bootstrap: joinForm.bootstrap.trim() || undefined,
      }),
    );
    await stopDiscoveryQuietly();
  } catch (e) {
    message.error("加入房间失败：" + errMsg(e));
  } finally {
    busy.value = false;
  }
}

async function onLeaveRoom() {
  busy.value = true;
  try {
    await lanChatLeaveRoom();
    // 离开即清空组件内消息（不落盘）。
    applyRoomState(null);
    await startDiscoveryQuietly();
  } catch (e) {
    message.error("离开房间失败：" + errMsg(e));
  } finally {
    busy.value = false;
  }
}

async function onSend() {
  const text = draft.value.trim();
  if (!text || sending.value) return;
  sending.value = true;
  try {
    await lanChatSendMessage(text);
    draft.value = "";
  } catch (e) {
    message.error("发送失败：" + errMsg(e));
  } finally {
    sending.value = false;
  }
}

function onMessage(m: LanChatMessage) {
  if (seenMessageIds.has(m.messageId)) return;
  seenMessageIds.add(m.messageId);
  messages.value.push(m);
  void nextTick(() => {
    const el = msgListEl.value;
    if (el) el.scrollTop = el.scrollHeight;
  });
}

/** timestamp 兼容秒 / 毫秒两种 epoch。 */
function formatTime(ts: number): string {
  const d = new Date(ts < 1e12 ? ts * 1000 : ts);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  return `${hh}:${mm}:${ss}`;
}

async function startDiscoveryQuietly() {
  try {
    await lanChatStartDiscovery();
  } catch (e) {
    message.error("启动附近房间发现失败：" + errMsg(e));
  }
}

async function stopDiscoveryQuietly() {
  try {
    await lanChatStopDiscovery();
  } catch (e) {
    message.error("停止附近房间发现失败：" + errMsg(e));
  }
}

onMounted(async () => {
  unlisteners.push(
    await listen<LanChatRoomState>("lan_chat_room_state", (e) =>
      applyRoomState(e.payload),
    ),
    await listen<LanChatMessage>("lan_chat_message", (e) => onMessage(e.payload)),
    await listen<LanChatNearbyRoom[]>(
      "lan_chat_rooms",
      (e) => (nearbyRooms.value = e.payload),
    ),
    await listen<LanChatError>("lan_chat_error", (e) =>
      message.error(e.payload.message),
    ),
  );

  // 恢复状态：用户切走再切回工具时房间可能还开着。
  try {
    const state = await lanChatRoomState();
    applyRoomState(state);
    if (!state) await startDiscoveryQuietly();
  } catch (e) {
    message.error("获取房间状态失败：" + errMsg(e));
  }
});

onUnmounted(() => {
  for (const unlisten of unlisteners) unlisten();
  unlisteners.length = 0;
  // 组件卸载即停止发现广播；若仍在房间内，后端房间继续保留（切回时可恢复）。
  void lanChatStopDiscovery().catch(() => {});
});
</script>

<style scoped>
.lan-chat-tool {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  height: 100%;
}

.entry-tabs {
  max-width: 560px;
}

.entry-form {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-3);
  padding-top: var(--gw-space-2);
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
}

.field-label {
  font-size: var(--gw-text-md);
  color: var(--gw-text);
}

.secret-row {
  display: flex;
  gap: var(--gw-space-2);
}

.bootstrap-row {
  display: flex;
  gap: var(--gw-space-2);
}

.share-panel {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  max-width: 420px;
}

.share-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--gw-space-2);
}

.share-value {
  font-size: var(--gw-text-sm);
  word-break: break-all;
}

.hint {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}

.mono {
  font-family: var(--gw-font-mono);
}

.mono-input :deep(input) {
  font-family: var(--gw-font-mono);
}

.nearby-list {
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  max-height: 180px;
  overflow-y: auto;
}

.nearby-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: var(--gw-space-2);
  padding: var(--gw-space-2) var(--gw-space-3);
  border: 1px solid var(--gw-border);
  border-radius: var(--gw-radius-md);
  cursor: pointer;
}

.nearby-item:hover {
  background: var(--gw-bg-hover);
}

.nearby-item.active {
  border-color: var(--gw-accent);
}

.nearby-name {
  font-size: var(--gw-text-md);
}

.nearby-addr {
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
}

.room-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--gw-space-2);
  padding-bottom: var(--gw-space-2);
  border-bottom: 1px solid var(--gw-border);
}

.room-title {
  display: flex;
  align-items: center;
  gap: var(--gw-space-2);
}

.room-name {
  font-size: var(--gw-text-lg);
  font-weight: 600;
}

.secure-tag {
  cursor: pointer;
}

.room-meta {
  display: flex;
  align-items: center;
  gap: var(--gw-space-3);
}

.room-body {
  display: flex;
  gap: var(--gw-space-3);
  flex: 1;
  min-height: 320px;
}

.chat-area {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  gap: var(--gw-space-2);
}

.msg-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-2);
  padding: var(--gw-space-2);
  background: var(--gw-bg-hover);
  border-radius: var(--gw-radius-md);
}

.msg-empty {
  margin: auto;
}

.msg-row {
  display: flex;
}

.msg-row.mine {
  justify-content: flex-end;
}

.msg-bubble {
  max-width: 70%;
  padding: var(--gw-space-2) var(--gw-space-3);
  background: var(--gw-bg-panel);
  border-radius: var(--gw-radius-md);
}

.msg-row.mine .msg-bubble {
  background: var(--gw-accent);
  color: var(--gw-bg-panel);
}

.msg-meta {
  display: flex;
  gap: var(--gw-space-2);
  align-items: baseline;
}

.msg-sender {
  font-size: var(--gw-text-sm);
  font-weight: 600;
}

.msg-time {
  font-size: var(--gw-text-xs);
  color: var(--gw-text-dim);
}

.msg-row.mine .msg-time {
  color: inherit;
  opacity: 0.75;
}

.msg-content {
  font-size: var(--gw-text-md);
  white-space: pre-wrap;
  word-break: break-word;
  margin-top: var(--gw-space-1);
}

.input-row {
  display: flex;
  gap: var(--gw-space-2);
  align-items: flex-end;
}

.member-list {
  width: 200px;
  flex-shrink: 0;
  border-left: 1px solid var(--gw-border);
  padding-left: var(--gw-space-3);
  display: flex;
  flex-direction: column;
  gap: var(--gw-space-1);
  overflow-y: auto;
}

.member-header {
  font-size: var(--gw-text-sm);
  color: var(--gw-text-dim);
  padding-bottom: var(--gw-space-1);
}

.member-item {
  display: flex;
  align-items: center;
  gap: var(--gw-space-1);
  font-size: var(--gw-text-md);
}

.member-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--gw-success);
  flex-shrink: 0;
}

.member-self {
  color: var(--gw-text-dim);
  font-size: var(--gw-text-sm);
}
</style>
