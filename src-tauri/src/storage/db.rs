use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use rusqlite::{params, Connection, OptionalExtension, Result};
use crate::storage::models::{Contact, LocalProfile, Message};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.create_tables()?;
        Ok(db)
    }

    pub fn create_tables(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS local_profile (
                node_id TEXT PRIMARY KEY,
                secret_key_bytes BLOB NOT NULL,
                nickname TEXT NOT NULL,
                avatar_hash TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS contacts (
                node_id TEXT PRIMARY KEY,
                nickname TEXT NOT NULL,
                avatar_path TEXT,
                custom_volume REAL DEFAULT 1.0,
                last_seen INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                chat_id TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                content TEXT NOT NULL,
                attachment_hash TEXT,
                attachment_name TEXT,
                timestamp INTEGER NOT NULL,
                delivered INTEGER DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    // --- Local Profile CRUD ---

    pub fn get_local_profile(&self) -> Result<Option<LocalProfile>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT node_id, secret_key_bytes, nickname, avatar_hash FROM local_profile LIMIT 1",
        )?;
        let profile = stmt
            .query_row([], |row| {
                Ok(LocalProfile {
                    node_id: row.get(0)?,
                    secret_key_bytes: row.get(1)?,
                    nickname: row.get(2)?,
                    avatar_hash: row.get(3)?,
                })
            })
            .optional()?;
        Ok(profile)
    }

    pub fn save_local_profile(&self, profile: &LocalProfile) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO local_profile (node_id, secret_key_bytes, nickname, avatar_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                profile.node_id,
                profile.secret_key_bytes,
                profile.nickname,
                profile.avatar_hash
            ],
        )?;
        Ok(())
    }

    pub fn update_profile(&self, nickname: &str, avatar_hash: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE local_profile SET nickname = ?1, avatar_hash = ?2",
            params![nickname, avatar_hash],
        )?;
        Ok(())
    }

    // --- Contacts CRUD ---

    pub fn insert_or_update_contact(&self, contact: &Contact) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO contacts (node_id, nickname, avatar_path, custom_volume, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                contact.node_id,
                contact.nickname,
                contact.avatar_path,
                contact.custom_volume,
                contact.last_seen
            ],
        )?;
        Ok(())
    }

    /// Handles profile handshake from a peer: auto-updates or creates a contact entry in SQLite.
    /// Preserves user custom volume setting if the contact already exists.
    pub fn handle_profile_handshake(
        &self,
        peer_node_id: &str,
        nickname: &str,
        avatar_hash: Option<&str>,
    ) -> Result<Contact> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let existing = self.get_contact(peer_node_id)?;
        let custom_volume = existing.as_ref().map(|c| c.custom_volume).unwrap_or(1.0);

        let final_nickname = if nickname.trim().is_empty() {
            if let Some(ref ex) = existing {
                ex.nickname.clone()
            } else {
                let short_id = if peer_node_id.len() >= 4 {
                    &peer_node_id[..4]
                } else {
                    peer_node_id
                };
                format!("Peer_{}", short_id)
            }
        } else {
            nickname.trim().to_string()
        };

        let final_avatar = if avatar_hash.is_some() {
            avatar_hash.map(|s| s.to_string())
        } else if let Some(ref ex) = existing {
            ex.avatar_path.clone()
        } else {
            None
        };

        let contact = Contact {
            node_id: peer_node_id.to_string(),
            nickname: final_nickname,
            avatar_path: final_avatar,
            custom_volume,
            last_seen: now,
        };

        self.insert_or_update_contact(&contact)?;
        Ok(contact)
    }

    pub fn get_contacts(&self) -> Result<Vec<Contact>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT node_id, nickname, avatar_path, custom_volume, last_seen
             FROM contacts
             ORDER BY nickname ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Contact {
                node_id: row.get(0)?,
                nickname: row.get(1)?,
                avatar_path: row.get(2)?,
                custom_volume: row.get(3)?,
                last_seen: row.get(4)?,
            })
        })?;

        let mut contacts = Vec::new();
        for contact in rows {
            contacts.push(contact?);
        }
        Ok(contacts)
    }

    pub fn get_contact(&self, node_id: &str) -> Result<Option<Contact>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT node_id, nickname, avatar_path, custom_volume, last_seen
             FROM contacts
             WHERE node_id = ?1",
        )?;
        let contact = stmt
            .query_row(params![node_id], |row| {
                Ok(Contact {
                    node_id: row.get(0)?,
                    nickname: row.get(1)?,
                    avatar_path: row.get(2)?,
                    custom_volume: row.get(3)?,
                    last_seen: row.get(4)?,
                })
            })
            .optional()?;
        Ok(contact)
    }

    pub fn delete_contact(&self, node_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM contacts WHERE node_id = ?1", params![node_id])?;
        Ok(())
    }

    pub fn update_contact_volume(&self, node_id: &str, volume: f64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contacts SET custom_volume = ?1 WHERE node_id = ?2",
            params![volume, node_id],
        )?;
        Ok(())
    }

    pub fn update_contact_last_seen(&self, node_id: &str, timestamp: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contacts SET last_seen = ?1 WHERE node_id = ?2",
            params![timestamp, node_id],
        )?;
        Ok(())
    }

    // --- Messages CRUD ---

    pub fn insert_message(&self, message: &Message) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO messages (id, chat_id, sender_id, content, attachment_hash, attachment_name, timestamp, delivered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                message.id,
                message.chat_id,
                message.sender_id,
                message.content,
                message.attachment_hash,
                message.attachment_name,
                message.timestamp,
                message.delivered
            ],
        )?;
        Ok(())
    }

    pub fn get_messages_for_chat(&self, chat_id: &str, limit: usize) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, sender_id, content, attachment_hash, attachment_name, timestamp, delivered
             FROM messages
             WHERE chat_id = ?1
             ORDER BY timestamp ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![chat_id, limit as i64], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_id: row.get(2)?,
                content: row.get(3)?,
                attachment_hash: row.get(4)?,
                attachment_name: row.get(5)?,
                timestamp: row.get(6)?,
                delivered: row.get(7)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    pub fn mark_message_delivered(&self, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET delivered = 1 WHERE id = ?1",
            params![message_id],
        )?;
        Ok(())
    }

    /// Fetches all messages across all chats/contacts (for device backup/sync).
    pub fn get_all_messages(&self) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, sender_id, content, attachment_hash, attachment_name, timestamp, delivered
             FROM messages
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_id: row.get(2)?,
                content: row.get(3)?,
                attachment_hash: row.get(4)?,
                attachment_name: row.get(5)?,
                timestamp: row.get(6)?,
                delivered: row.get(7)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    /// Fetches all undelivered messages across all chats/contacts.
    pub fn get_all_undelivered_messages(&self) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, sender_id, content, attachment_hash, attachment_name, timestamp, delivered
             FROM messages
             WHERE delivered = 0
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_id: row.get(2)?,
                content: row.get(3)?,
                attachment_hash: row.get(4)?,
                attachment_name: row.get(5)?,
                timestamp: row.get(6)?,
                delivered: row.get(7)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    /// Fetches all undelivered messages queued for a given contact / node_id.
    pub fn get_undelivered_messages(&self, node_id: &str) -> Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, sender_id, content, attachment_hash, attachment_name, timestamp, delivered
             FROM messages
             WHERE delivered = 0 AND chat_id = ?1
             ORDER BY timestamp ASC",
        )?;
        let rows = stmt.query_map(params![node_id], |row| {
            Ok(Message {
                id: row.get(0)?,
                chat_id: row.get(1)?,
                sender_id: row.get(2)?,
                content: row.get(3)?,
                attachment_hash: row.get(4)?,
                attachment_name: row.get(5)?,
                timestamp: row.get(6)?,
                delivered: row.get(7)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in rows {
            messages.push(msg?);
        }
        Ok(messages)
    }

    pub fn delete_message(&self, message_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
        Ok(())
    }

    // --- App Settings CRUD ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn get_all_settings(&self) -> Result<HashMap<String, String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT key, value FROM app_settings")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let mut map = HashMap::new();
        for r in rows {
            let (k, v) = r?;
            map.insert(k, v);
        }
        Ok(map)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profile_crud() {
        let db = Database::new_in_memory().expect("db init failed");
        assert!(db.get_local_profile().unwrap().is_none());

        let profile = LocalProfile {
            node_id: "test_node_id_1234".to_string(),
            secret_key_bytes: vec![1u8; 32],
            nickname: "Alice".to_string(),
            avatar_hash: Some("hash123".to_string()),
        };

        db.save_local_profile(&profile).unwrap();
        let loaded = db.get_local_profile().unwrap().expect("profile should exist");
        assert_eq!(loaded.node_id, "test_node_id_1234");
        assert_eq!(loaded.nickname, "Alice");
        assert_eq!(loaded.avatar_hash, Some("hash123".to_string()));

        db.update_profile("Alice_Updated", None).unwrap();
        let updated = db.get_local_profile().unwrap().unwrap();
        assert_eq!(updated.nickname, "Alice_Updated");
        assert_eq!(updated.avatar_hash, None);
    }

    #[test]
    fn test_contacts_crud() {
        let db = Database::new_in_memory().expect("db init failed");
        let contact = Contact {
            node_id: "peer_node_abc".to_string(),
            nickname: "Bob".to_string(),
            avatar_path: None,
            custom_volume: 1.0,
            last_seen: 12345678,
        };

        db.insert_or_update_contact(&contact).unwrap();
        let contacts = db.get_contacts().unwrap();
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].nickname, "Bob");

        db.update_contact_volume("peer_node_abc", 1.5).unwrap();
        let updated = db.get_contact("peer_node_abc").unwrap().unwrap();
        assert_eq!(updated.custom_volume, 1.5);

        db.delete_contact("peer_node_abc").unwrap();
        assert!(db.get_contact("peer_node_abc").unwrap().is_none());
    }

    #[test]
    fn test_messages_crud_and_undelivered() {
        let db = Database::new_in_memory().expect("db init failed");
        let msg1 = Message {
            id: "msg_1".to_string(),
            chat_id: "peer_target".to_string(),
            sender_id: "my_node".to_string(),
            content: "Hello!".to_string(),
            attachment_hash: None,
            attachment_name: None,
            timestamp: 100,
            delivered: 0,
        };
        let msg2 = Message {
            id: "msg_2".to_string(),
            chat_id: "peer_target".to_string(),
            sender_id: "my_node".to_string(),
            content: "Are you there?".to_string(),
            attachment_hash: None,
            attachment_name: None,
            timestamp: 200,
            delivered: 0,
        };

        db.insert_message(&msg1).unwrap();
        db.insert_message(&msg2).unwrap();

        let undelivered = db.get_undelivered_messages("peer_target").unwrap();
        assert_eq!(undelivered.len(), 2);

        db.mark_message_delivered("msg_1").unwrap();
        let remaining_undelivered = db.get_undelivered_messages("peer_target").unwrap();
        assert_eq!(remaining_undelivered.len(), 1);
        assert_eq!(remaining_undelivered[0].id, "msg_2");
    }

    #[test]
    fn test_handle_profile_handshake() {
        let db = Database::new_in_memory().expect("db init failed");
        let contact = db
            .handle_profile_handshake("peer_handshake_1", "Charlie", Some("hash_avatar_1"))
            .unwrap();
        assert_eq!(contact.nickname, "Charlie");
        assert_eq!(contact.avatar_path, Some("hash_avatar_1".to_string()));
        assert_eq!(contact.custom_volume, 1.0);

        // Update custom volume
        db.update_contact_volume("peer_handshake_1", 1.8).unwrap();

        // Subsequent handshake from Charlie with updated avatar, should preserve custom volume 1.8
        let updated = db
            .handle_profile_handshake("peer_handshake_1", "Charlie_Renamed", Some("hash_avatar_2"))
            .unwrap();
        assert_eq!(updated.nickname, "Charlie_Renamed");
        assert_eq!(updated.avatar_path, Some("hash_avatar_2".to_string()));
        assert_eq!(updated.custom_volume, 1.8);

        // Handshake with empty nickname should preserve Charlie_Renamed
        let empty_nick = db
            .handle_profile_handshake("peer_handshake_1", "", Some("hash_avatar_3"))
            .unwrap();
        assert_eq!(empty_nick.nickname, "Charlie_Renamed");
        assert_eq!(empty_nick.avatar_path, Some("hash_avatar_3".to_string()));
    }

    #[test]
    fn test_app_settings_crud() {
        let db = Database::new_in_memory().expect("db init failed");
        assert_eq!(db.get_setting("bypass_vpn").unwrap(), None);

        db.set_setting("bypass_vpn", "true").unwrap();
        assert_eq!(db.get_setting("bypass_vpn").unwrap(), Some("true".to_string()));

        db.set_setting("bypass_vpn", "false").unwrap();
        assert_eq!(db.get_setting("bypass_vpn").unwrap(), Some("false".to_string()));

        db.set_setting("rnnoise_enabled", "true").unwrap();
        db.set_setting("vad_threshold", "0.45").unwrap();

        let all = db.get_all_settings().unwrap();
        assert_eq!(all.get("bypass_vpn"), Some(&"false".to_string()));
        assert_eq!(all.get("rnnoise_enabled"), Some(&"true".to_string()));
        assert_eq!(all.get("vad_threshold"), Some(&"0.45".to_string()));
    }
}
