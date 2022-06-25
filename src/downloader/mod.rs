pub mod youtube;

use crate::bot::Notifier;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use std::fmt::Debug;
use std::sync::Arc;

#[async_trait]
pub trait Downloader: Debug + Send + Sync {
    fn probe_url(&self, url: &str) -> bool;
    async fn download(
        self: Arc<Self>,
        url: String,
        notifier: Notifier,
    ) -> anyhow::Result<BoxStream<'static, futures::io::Result<Bytes>>>;
}
