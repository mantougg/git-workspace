import { invoke } from "@tauri-apps/api/core";

/** 生成 256-bit 随机 Secret 的三种编码形式。 */
export interface LanChatSecret {
  hex: string;
  base64: string;
  base64Url: string;
}

/** 房间成员（全量快照中的一项）。 */
export interface LanChatMember {
  peerId: string;
  nickname: string;
  isSelf: boolean;
}

/** 房间状态快照（lan_chat_room_state 事件 / 命令返回值）。 */
export interface LanChatRoomState {
  roomId: string;
  roomName: string;
  nickname: string;
  peerId: string;
  port: number;
  connectedPeers: number;
  members: LanChatMember[];
}

/** 聊天消息（lan_chat_message 事件）。 */
export interface LanChatMessage {
  messageId: string;
  senderName: string;
  content: string;
  timestamp: number;
  mine: boolean;
}

/** 附近房间（lan_chat_rooms 事件，全量列表中的一项）。 */
export interface LanChatNearbyRoom {
  roomId: string;
  roomName: string;
  addr: string;
  port: number;
}

/** 后端主动上报的错误（lan_chat_error 事件）。 */
export interface LanChatError {
  message: string;
}

export function lanChatGenerateSecret() {
  return invoke<LanChatSecret>("lan_chat_generate_secret");
}

export function lanChatCreateRoom(args: {
  roomName: string;
  secret: string;
  nickname: string;
}) {
  return invoke<LanChatRoomState>("lan_chat_create_room", args);
}

export function lanChatJoinRoom(args: {
  roomId: string;
  secret: string;
  nickname: string;
  bootstrap?: string;
}) {
  return invoke<LanChatRoomState>("lan_chat_join_room", args);
}

export function lanChatLeaveRoom() {
  return invoke<void>("lan_chat_leave_room");
}

export function lanChatSendMessage(text: string) {
  return invoke<void>("lan_chat_send_message", { text });
}

export function lanChatRoomState() {
  return invoke<LanChatRoomState | null>("lan_chat_room_state");
}

export function lanChatStartDiscovery() {
  return invoke<void>("lan_chat_start_discovery");
}

export function lanChatStopDiscovery() {
  return invoke<void>("lan_chat_stop_discovery");
}

/** 本机局域网 IPv4 地址列表（房间头部分享监听地址用）。 */
export function lanChatLocalAddrs() {
  return invoke<string[]>("lan_chat_local_addrs");
}
