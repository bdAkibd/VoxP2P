use std::path::Path;
use rusqlite::{params, Connection, Result};
use crate::storage::models::{ChatMessage, Contact, QueuedMessage};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.create_tables()?;
        Ok(db)
    }

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.create_tables()?;
        Ok(db)
    }

    fn create_tables(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS contacts (
                node_id TEXT PRIMARY KEY,
                nickname TEXT NOT NULL,
                added_at INTEGER NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS chat_messages (
                id TEXT PRIMARY KEY,
                room_id TEXT NOT NULL,
                sender_node_id TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS queued_messages (
                id TEXT PRIMARY KEY,
                target_node_id TEXT NOT NULL,
                payload BLOB NOT NULL,
                attempts INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    pub fn insert_contact(&self, contact: &Contact) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO contacts (node_id, nickname, added_at) VALUES (?1, ?2, ?3)",
            params![contact.node_id, contact.nickname, contact.added_at],
        )?;
        Ok(())
    }

    pub fn get_contacts(&self) -> Result<Vec<Contact>> {
        let mut stmt = self.conn.prepare("SELECT node_id, nickname, added_at FROM contacts ORDER BY nickname ASC")?;
        let contact_iter = stmt.query_map([], |row| {
            Ok(Contact {
                node_id: row.get(0)?,
                nickname: row.get(1)?,
                added_at: row.get(2)?,
            })
        })?;

        let mut contacts = Vec::new();
        for contact in contact_iter {
            contacts.push(contact?);
        }
        Ok(contacts)
    }

    pub fn insert_chat_message(&self, msg: &ChatMessage) -> Result<()> {
        self.conn.execute(
            "INSERT INTO chat_messages (id, room_id, sender_node_id, content, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg.id, msg.room_id, msg.sender_node_id, msg.content, msg.timestamp],
        )?;
        Ok(())
    }

    pub fn get_room_messages(&self, room_id: &str, limit: usize) -> Result<Vec<ChatMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, room_id, sender_node_id, content, timestamp 
             FROM chat_messages 
             WHERE room_id = ?1 
             ORDER BY timestamp ASC 
             LIMIT ?2",
        )?;
        let msg_iter = stmt.query_map(params![room_id, limit as i64], |row| {
            Ok(ChatMessage {
                id: row.get(0)?,
                room_id: row.get(1)?,
                sender_node_id: row.get(2)?,
                content: row.get(3)?,
                timestamp: row.get(4)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in msg_iter {
            messages.push(msg?);
        }
        Ok(messages)
    }

    pub fn enqueue_message(&self, msg: &QueuedMessage) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO queued_messages (id, target_node_id, payload, attempts, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![msg.id, msg.target_node_id, msg.payload, msg.attempts, msg.created_at],
        )?;
        Ok(())
    }

    pub fn pop_queued_messages(&self, limit: usize) -> Result<Vec<QueuedMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, target_node_id, payload, attempts, created_at 
             FROM queued_messages 
             ORDER BY created_at ASC 
             LIMIT ?1",
        )?;
        let msg_iter = stmt.query_map(params![limit as i64], |row| {
            Ok(QueuedMessage {
                id: row.get(0)?,
                target_node_id: row.get(1)?,
                payload: row.get(2)?,
                attempts: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;

        let mut messages = Vec::new();
        for msg in msg_iter {
            messages.push(msg?);
        }
        Ok(messages)
    }

    pub fn remove_queued_message(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM queued_messages WHERE id = ?1", params![id])?;
        Ok(())
    }
}
