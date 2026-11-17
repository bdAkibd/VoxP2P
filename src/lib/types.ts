export interface UserProfile {
  node_id: string;
  nickname: string;
  avatar_hash?: string | null;
}

export interface Contact {
  node_id: string;
  nickname: string;
  avatar_path?: string | null;
  custom_volume: number;
  last_seen: number;
}

export interface PeerInfo {
  node_id: string;
  nickname: string;
  avatar_hash?: string | null;
  join_timestamp: number;
  is_host: boolean;
  is_muted: boolean;
  is_deafened: boolean;
}

export interface RoomState {
  room_id: string;
  host_node_id: string;
  peers: PeerInfo[];
  ticket?: string | null;
}

export interface Message {
  id: string;
  chat_id: string;
  sender_id: string;
  content: string;
  attachment_hash?: string | null;
  attachment_name?: string | null;
  timestamp: number;
  delivered: number;
}

export interface AudioDeviceInfo {
  name: string;
  is_default: boolean;
  channels: number;
  sample_rate: number;
}

export interface AudioDevicesResponse {
  input_devices: AudioDeviceInfo[];
  output_devices: AudioDeviceInfo[];
}

export interface TransferProgressPayload {
  hash: string;
  percent: number;
  bytes_downloaded: number;
  total_bytes: number;
  is_complete: boolean;
}

export interface HostChangedPayload {
  room_id: string;
  old_host_id: string;
  new_host_id: string;
}

export interface AttachFileResponse {
  id: string;
  chat_id: string;
  text: string;
  attachment_hash: string;
  attachment_name: string;
  attachment_size: number;
  mime_type: string;
  ticket: string;
}

export interface DeviceSyncTicket {
  ticket: string;
  blob_hash: string;
  contacts_count: number;
  messages_count: number;
  exported_at: number;
}

export interface ImportSyncResult {
  contacts_imported: number;
  messages_imported: number;
}
