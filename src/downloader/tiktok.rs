use crate::bot::Notifier;
use crate::downloader::Downloader;
use anyhow::anyhow;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use once_cell::sync::Lazy;
use regex::Regex;
use std::sync::Arc;
use url::Url;

pub static URL_PATTERNS: [Lazy<Regex>; 2] = [
    Lazy::new(|| {
        Regex::new(r"^(https?://)?((www|m)\.)?tiktok\.com/[@a-zA-Z0-9-_]+/video/[0-9]+(\?.*)?$")
            .unwrap()
    }),
    Lazy::new(|| Regex::new(r"^https://v[mt]\.tiktok\.com/[a-zA-Z0-9]{9}/.*$").unwrap()),
];

#[derive(Debug)]
pub struct TikTokDownloader {}

#[async_trait]
impl Downloader for TikTokDownloader {
    fn probe_url(&self, url: &Url) -> bool {
        URL_PATTERNS
            .iter()
            .any(|pattern| pattern.is_match(url.as_str()))
    }

    fn link_text(&self) -> &'static str {
        "🔗 TikTok"
    }

    async fn download(
        self: Arc<Self>,
        url: Url,
        notifier: Notifier,
    ) -> anyhow::Result<BoxStream<'static, std::io::Result<Bytes>>> {
        Err(anyhow!("Not implemented =("))
    }
}
