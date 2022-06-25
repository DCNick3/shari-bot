pub mod youtube;

use crate::bot::Notifier;
use async_trait::async_trait;
use std::fmt::Debug;

#[async_trait]
pub trait Downloader: Debug + Send + Sync {
    fn probe_url(&self, url: &str) -> bool;
    async fn download(&self, url: &str, notifier: Notifier);
}
