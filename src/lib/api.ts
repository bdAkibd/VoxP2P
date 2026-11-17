import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  UserProfile,
  Contact,
  RoomState,
  Message,
  AudioDevicesResponse,
  TransferProgressPayload,
  HostChangedPayload,
  AttachFileResponse,
  DeviceSyncTicket,
  ImportSyncResult,
  DirectCallRingPayload,
  MessageDeliveredPayload,
  LocalInputLevelPayload,
  DeviceSyncTicketPayload,
  DeviceSyncImportResult,
  SyncHostCompletedPayload,
  SyncImportCompletedPayload,
  MessageEditedPayload,
  MessageDeletedPayload,
} from "./types";

export const api = {
  async getMyProfile(): Promise<UserProfile> {
    return await invoke<UserProfile>("get_my_profile");
  },

  async updateProfile(nickname: string, avatarPath?: string | null): Promise<UserProfile> {
    return await invoke<UserProfile>("update_profile", {
      nickname,
      avatarPath: avatarPath ?? null,
    });
  },

  async updateProfileAvatar(avatarBytes: number[] | Uint8Array): Promise<UserProfile> {
    const bytesArray = Array.from(avatarBytes);
    return await invoke<UserProfile>("update_profile_avatar", { avatarBytes: bytesArray });
  },

  async getAvatar(avatarHash: string): Promise<string | null> {
    return await invoke<string | null>("get_avatar", { avatarHash });
  },

  async addContact(nodeId: string): Promise<Contact> {
    return await invoke<Contact>("add_contact", { nodeId });
  },

  async getNodeStatus(): Promise<any> {
    return await invoke("get_node_status");
  },

  async getContacts(): Promise<Contact[]> {
    return await invoke<Contact[]>("get_contacts");
  },

  async getUndeliveredMessages(nodeId: string): Promise<Message[]> {
    return await invoke<Message[]>("get_undelivered_messages", { nodeId });
  },

  async listAudioDevices(): Promise<AudioDevicesResponse> {
    return await invoke<AudioDevicesResponse>("list_audio_devices");
  },

  async setInputDevice(name: string): Promise<void> {
    return await invoke("set_input_device", { name });
  },

  async setOutputDevice(name: string): Promise<void> {
    return await invoke("set_output_device", { name });
  },

  async setMute(muted: boolean): Promise<void> {
    return await invoke("set_mute", { muted });
  },

  async setDeafen(deafened: boolean): Promise<void> {
    return await invoke("set_deafen", { deafened });
  },

  async setPeerVolume(nodeId: string, volume: number): Promise<void> {
    return await invoke("set_peer_volume", { nodeId, volume });
  },

  async setMicrophoneGain(gain: number): Promise<void> {
    return await invoke("set_microphone_gain", { gain });
  },

  async setMasterVolume(volume: number): Promise<void> {
    return await invoke("set_master_volume", { volume });
  },

  async setRNNoise(enabled: boolean): Promise<void> {
    return await invoke("set_rnnoise", { enabled });
  },

  async setMicTest(enabled: boolean): Promise<void> {
    return await invoke("set_mic_test", { enabled });
  },

  async setMeterActive(active: boolean): Promise<void> {
    return await invoke("set_meter_active", { active });
  },

  async setVadThreshold(threshold: number): Promise<void> {
    return await invoke("set_vad_threshold", { threshold });
  },

  async setBypassVpn(bypass: boolean): Promise<void> {
    return await invoke("set_bypass_vpn", { bypass });
  },

  async getAppSettings(): Promise<Record<string, string>> {
    return await invoke<Record<string, string>>("get_app_settings");
  },

  async setAppSetting(key: string, value: string): Promise<void> {
    return await invoke("set_app_setting", { key, value });
  },

  async transferHost(targetNodeId: string): Promise<void> {
    return await invoke("transfer_host", { targetNodeId });
  },

  async getRoomState(): Promise<RoomState | null> {
    return await invoke<RoomState | null>("get_room_state");
  },

  async getRoomTicket(): Promise<string | null> {
    return await invoke<string | null>("get_room_ticket");
  },

  async startDirectCall(recipientNodeId: string): Promise<RoomState> {
    return await invoke<RoomState>("start_direct_call", { recipientNodeId });
  },

  async joinRoom(roomId: string, hostId?: string | null): Promise<RoomState> {
    return await invoke<RoomState>("join_room", {
      roomId,
      hostId: hostId ?? null,
    });
  },

  async leaveRoom(): Promise<void> {
    return await invoke("leave_room");
  },

  async attachFile(
    filePath: string,
    chatId?: string | null,
    text?: string | null
  ): Promise<AttachFileResponse> {
    return await invoke<AttachFileResponse>("attach_file", {
      filePath,
      chatId: chatId ?? null,
      text: text ?? null,
    });
  },

  async downloadAttachment(
    nodeId: string,
    blobHash: string,
    savePath: string
  ): Promise<void> {
    return await invoke("download_attachment", {
      nodeId,
      blobHash,
      savePath,
    });
  },

  async sendRoomMessage(
    content: string,
    attachmentHash?: string | null,
    attachmentName?: string | null
  ): Promise<Message> {
    return await invoke<Message>("send_room_message", {
      content,
      attachmentHash: attachmentHash ?? null,
      attachmentName: attachmentName ?? null,
    });
  },

  async sendDirectMessage(
    recipientNodeId: string,
    content: string,
    attachmentHash?: string | null,
    attachmentName?: string | null
  ): Promise<Message> {
    return await invoke<Message>("send_direct_message", {
      recipientNodeId,
      content,
      attachmentHash: attachmentHash ?? null,
      attachmentName: attachmentName ?? null,
    });
  },

  async sendMessage(
    chatId: string,
    content: string,
    attachmentHash?: string | null,
    attachmentName?: string | null
  ): Promise<Message> {
    return await invoke<Message>("send_message", {
      chatId,
      content,
      attachmentHash: attachmentHash ?? null,
      attachmentName: attachmentName ?? null,
    });
  },

  async getMessages(chatId: string, limit: number = 50): Promise<Message[]> {
    return await invoke<Message[]>("get_messages", {
      chatId,
      limit,
    });
  },

  async syncDevices(): Promise<DeviceSyncTicket> {
    return await invoke<DeviceSyncTicket>("sync_devices");
  },

  async importSyncTicket(ticket: string): Promise<ImportSyncResult> {
    return await invoke<ImportSyncResult>("import_sync_ticket", { ticket });
  },

  async startDeviceSyncHost(): Promise<DeviceSyncTicketPayload> {
    return await invoke<DeviceSyncTicketPayload>("start_device_sync_host");
  },

  async importFromSyncQr(qrString: string): Promise<DeviceSyncImportResult> {
    return await invoke<DeviceSyncImportResult>("import_from_sync_qr", { qrString });
  },

  async deleteContact(nodeId: string): Promise<void> {
    return await invoke("delete_contact", { nodeId });
  },

  async editMessage(messageId: string, newContent: string): Promise<void> {
    return await invoke("edit_message", { messageId, newContent });
  },

  async deleteMessage(messageId: string, forEveryone: boolean = false): Promise<void> {
    return await invoke("delete_message", { messageId, forEveryone });
  },

  // Event Listeners
  onAudioLevels(callback: (levels: Record<string, number>) => void): Promise<UnlistenFn> {
    return listen<Record<string, number>>("audio://levels", (event) => {
      callback(event.payload);
    });
  },

  onHostChanged(callback: (payload: HostChangedPayload) => void): Promise<UnlistenFn> {
    return listen<HostChangedPayload>("room://host-changed", (event) => {
      callback(event.payload);
    });
  },

  onPeersUpdated(callback: (payload: RoomState) => void): Promise<UnlistenFn> {
    return listen<RoomState>("room://peers-updated", (event) => {
      callback(event.payload);
    });
  },

  onChatMessage(callback: (msg: Message) => void): Promise<UnlistenFn> {
    return listen<any>("voxp2p://chat-message", (event) => {
      const msg = event.payload?.message || event.payload;
      callback(msg);
    });
  },

  onChatMessageReceived(callback: (msg: Message) => void): Promise<UnlistenFn> {
    return listen<Message>("chat://message-received", (event) => {
      callback(event.payload);
    });
  },

  onChatMessageDelivered(callback: (payload: MessageDeliveredPayload) => void): Promise<UnlistenFn> {
    return listen<MessageDeliveredPayload>("chat://message-delivered", (event) => {
      callback(event.payload);
    });
  },

  onIncomingCall(callback: (payload: DirectCallRingPayload) => void): Promise<UnlistenFn> {
    return listen<DirectCallRingPayload>("call://incoming-ring", (event) => {
      callback(event.payload);
    });
  },

  onTransferProgress(callback: (payload: TransferProgressPayload) => void): Promise<UnlistenFn> {
    return listen<TransferProgressPayload>("transfer://progress", (event) => {
      callback(event.payload);
    });
  },

  onLocalInputLevel(callback: (payload: LocalInputLevelPayload) => void): Promise<UnlistenFn> {
    return listen<LocalInputLevelPayload>("audio://local-input-level", (event) => {
      callback(event.payload);
    });
  },

  onSyncHostCompleted(callback: (payload: SyncHostCompletedPayload) => void): Promise<UnlistenFn> {
    return listen<SyncHostCompletedPayload>("sync://host-completed", (event) => {
      callback(event.payload);
    });
  },

  onSyncImportCompleted(callback: (payload: SyncImportCompletedPayload) => void): Promise<UnlistenFn> {
    return listen<SyncImportCompletedPayload>("sync://import-completed", (event) => {
      callback(event.payload);
    });
  },

  onProfileUpdated(callback: (p: UserProfile) => void): Promise<UnlistenFn> {
    return listen<UserProfile>("profile://updated", (event) => {
      callback(event.payload);
    });
  },

  onContactsUpdated(callback: () => void): Promise<UnlistenFn> {
    return listen("contacts://updated", () => {
      callback();
    });
  },

  onChatMessageEdited(callback: (payload: MessageEditedPayload) => void): Promise<UnlistenFn> {
    return listen<MessageEditedPayload>("chat://message-edited", (event) => {
      callback(event.payload);
    });
  },

  onChatMessageDeleted(callback: (payload: MessageDeletedPayload) => void): Promise<UnlistenFn> {
    return listen<MessageDeletedPayload>("chat://message-deleted", (event) => {
      callback(event.payload);
    });
  },
};
