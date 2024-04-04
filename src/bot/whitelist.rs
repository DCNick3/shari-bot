use crate::whatever::Whatever;
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{collections::HashMap, io, path::PathBuf};
use tokio::fs::read_to_string;

use super::UserId;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd, Clone, Hash)]
pub struct UserInfo {
    pub access_hash: i64,
}

/// Keeps track of allowed users storing updates on the disk.
pub struct Whitelist {
    /// Path to the file to store state into.
    storage_path: PathBuf,
    allowed_users: HashMap<UserId, UserInfo>,
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
        match read_to_string(&me.storage_path).await {
            Ok(data) => {
                me.allowed_users = serde_json::from_str::<HashMap<UserId, UserInfo>>(&data)
                    .whatever_context("Deserializing whitelist")?;
            }
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => (),
                _ => Err(e).whatever_context("Reading from file")?,
            },
        }
        Ok(me)
    }

    async fn store_into_disk(&mut self) -> Result<(), Whatever> {
        let list_serialized =
            serde_json::to_vec(&self.allowed_users).whatever_context("Serializing whitelist")?;
        if let Some(parent) = self.storage_path.as_path().parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .whatever_context("Creating folder")?;
        }
        tokio::fs::write(self.storage_path.as_path(), list_serialized)
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
    pub async fn insert(&mut self, user: UserId, info: UserInfo) -> Result<bool, Whatever> {
        let updated = self.allowed_users.insert(user, info.clone());
        self.store_into_disk()
            .await
            .whatever_context("Storing state on disk")?;
        Ok(updated.is_none())
    }

    /// Remove a user from the whitelist.
    ///
    /// Returns removed access hash on success. Updates disk state if applicable.
    pub async fn remove(&mut self, user: UserId) -> Result<Option<UserInfo>, Whatever> {
        let updated = self.allowed_users.remove(&user);
        if updated.is_some() {
            self.store_into_disk()
                .await
                .whatever_context("Storing state on disk")?;
        }
        Ok(updated)
    }

    /// Returns user info given user id.
    #[inline]
    #[allow(unused)]
    pub fn get(&self, user: &UserId) -> Option<&UserInfo> {
        self.allowed_users.get(user)
    }

    /// Returns true if the list contains a user.
    #[inline]
    pub fn contains(&self, user: &UserId) -> bool {
        self.allowed_users.contains_key(user)
    }

    #[inline]
    pub fn users(&self) -> &HashMap<UserId, UserInfo> {
        &self.allowed_users
    }
}
