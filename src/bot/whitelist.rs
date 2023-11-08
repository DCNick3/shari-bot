use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::UserId;

// pub trait WhitelistStorage {
//     kjjn
// }

/// Keeps track of allowed users storing updates on the disk.
pub struct Whitelist {
    storage_path: PathBuf,
    allowed_users: HashSet<UserId>,
}

impl Whitelist {
    pub fn new_empty(path: PathBuf) -> Self {
        Self {
            storage_path: path,
            allowed_users: Default::default(),
        }
    }

    /// Loads state from the storage, overriding everything. Typically should be used for
    /// startup initialization.
    pub async fn load_from_disk(&mut self) -> Result<()> {
        let mut data = Vec::new();
        File::open(self.storage_path.as_path())
            .await
            .context("Opening file")?
            .read_to_end(&mut data)
            .await
            .context("Reading file")?;
        self.allowed_users =
            serde_json::from_slice::<HashSet<UserId>>(&data[..]).context("Deserializing data")?;
        Ok(())
    }

    async fn store_into_disk(&mut self) -> Result<()> {
        let list_serialized =
            serde_json::to_vec(&self.allowed_users).context("Serializing data")?;
        File::open(self.storage_path.as_path())
            .await
            .context("Opening file")?
            .write_all(&list_serialized[..])
            .await
            .context("Writing to file")?;
        Ok(())
    }

    fn insert() {}
    fn remove() {}
    fn contains() {}
}
