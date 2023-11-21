use snafu::{ResultExt, Whatever};
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::UserId;

/// Keeps track of allowed users storing updates on the disk.
pub struct Whitelist {
    /// Path to the file to store state into.
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

    /// Loads state from the storage
    pub async fn new_from_disk(path: PathBuf) -> Result<Self, Whatever> {
        let mut me = Self::new_empty(path);
        let mut data = Vec::new();
        if me.storage_path.is_file() {
            let mut file = OpenOptions::new()
                .read(true)
                .open(me.storage_path.as_path())
                .await
                .whatever_context("Opening file")?;
            file.read_to_end(&mut data)
                .await
                .whatever_context("Reading file")?;
            me.allowed_users = serde_json::from_slice::<HashSet<UserId>>(&data[..])
                .whatever_context("Deserializing data")?;
        }
        Ok(me)
    }

    async fn store_into_disk(&mut self) -> Result<(), Whatever> {
        let list_serialized =
            serde_json::to_vec(&self.allowed_users).whatever_context("Serializing data")?;
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(self.storage_path.as_path())
            .await
            .whatever_context("Opening file")?;
        file.write_all(&list_serialized[..])
            .await
            .whatever_context("Writing to file")?;
        Ok(())
    }

    /// Adds a user to the whitelist.
    ///
    /// Returns whether the user was newly inserted. That is:
    ///
    /// - If the set did not previously contain this value, `true` is returned,
    ///     updates are stored in the disk.
    /// - If the set already contained this value, `false` is returned.
    pub async fn insert(&mut self, user: UserId) -> Result<bool, Whatever> {
        let updated = self.allowed_users.insert(user);
        if updated {
            self.store_into_disk()
                .await
                .whatever_context("Storing state on disk")?;
        }
        Ok(updated)
    }

    /// Remove a user from the whitelist.
    ///
    /// Returns whether the user was removed. If yes, the list is propagated
    /// on disk.
    pub async fn remove(&mut self, user: UserId) -> Result<bool, Whatever> {
        let updated = self.allowed_users.remove(&user);
        if updated {
            self.store_into_disk()
                .await
                .whatever_context("Storing state on disk")?;
        }
        Ok(updated)
    }

    /// Returns true if the list contains a user.
    pub fn contains(&self, user: UserId) -> bool {
        self.allowed_users.contains(&user)
    }

    pub fn users(&self) -> &HashSet<UserId> {
        &self.allowed_users
    }
}
