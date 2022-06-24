use crate::bot::Notifier;
use async_trait::async_trait;

#[async_trait]
pub trait Downloader {
    fn probe_url(&self, url: &str) -> bool;
    async fn download(&self, url: &str, notifier: Notifier);
}
