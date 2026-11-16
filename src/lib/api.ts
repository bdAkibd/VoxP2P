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

  async setBypassVpn(bypass: boolean): Promise<void> {
    return await invoke("set_bypass_vpn", { bypass });
  },

  async transferHost(targetNodeId: string): Promise<void> {
    return await invoke("transfer_host", { targetNodeId });
  },

  async getRoomState(): Promise<RoomState | null> {
    return await invoke<RoomState | null>("get_room_state");
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

  onChatMessage(callback: (msg: Message) => void): Promise<UnlistenFn> {
    return listen<any>("chat://message", (event) => {
      const msg = event.payload?.message || event.payload;
      callback(msg);
    });
  },

  onTransferProgress(callback: (payload: TransferProgressPayload) => void): Promise<UnlistenFn> {
    return listen<TransferProgressPayload>("transfer://progress", (event) => {
      callback(event.payload);
    });
  },
};
