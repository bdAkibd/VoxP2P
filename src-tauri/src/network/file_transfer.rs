use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobTransferProgress {
    pub hash: String,
    pub filename: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub is_complete: bool,
}

pub struct FileTransferManager {
    pub active_transfers: Vec<BlobTransferProgress>,
}

impl Default for FileTransferManager {
    fn default() -> Self {
        Self::new()
    }
}

impl FileTransferManager {
    pub fn new() -> Self {
        Self {
            active_transfers: Vec::new(),
        }
    }

    pub fn track_transfer(&mut self, progress: BlobTransferProgress) {
        if let Some(existing) = self.active_transfers.iter_mut().find(|t| t.hash == progress.hash) {
            *existing = progress;
        } else {
            self.active_transfers.push(progress);
        }
    }

    pub fn get_progress(&self, hash: &str) -> Option<&BlobTransferProgress> {
        self.active_transfers.iter().find(|t| t.hash == hash)
    }

    pub fn remove_transfer(&mut self, hash: &str) {
        self.active_transfers.retain(|t| t.hash != hash);
    }
}
