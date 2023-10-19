pub mod tiktok;
pub mod youtube;

use crate::bot::{Notifier, ProgressInfo};
use crate::{StreamExt, TryStreamExt};
use anyhow::Context as _;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::Stream;
use pin_project_lite::pin_project;
use reqwest::Client;
use std::fmt::Debug;
use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tracing::{debug, warn};
use url::Url;

#[async_trait]
pub trait Downloader: Debug + Send + Sync {
    fn probe_url(&self, url: &url::Url) -> bool;
    fn link_text(&self) -> &'static str;
    async fn download(
        self: Arc<Self>,
        url: url::Url,
        notifier: Notifier,
    ) -> anyhow::Result<(BoxStream<'static, futures::io::Result<Bytes>>, u64)>;
}

pin_project! {
    struct ProgressStream<T: Stream<Item = anyhow::Result<Bytes>>> {
        #[pin]
        stream: T,
        size: Option<u64>,
        byte_counter: u64,
        notifier: Notifier,
    }
}

impl<T: Stream<Item = anyhow::Result<Bytes>>> ProgressStream<T> {
    pub fn new(stream: T, size: Option<u64>, notifier: Notifier) -> Self {
        Self {
            stream,
            size,
            byte_counter: 0,
            notifier,
        }
    }
}

impl<T: Stream<Item = anyhow::Result<Bytes>>> Stream for ProgressStream<T> {
    type Item = anyhow::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_ = self.project();
        match self_.stream.poll_next(cx) {
            Poll::Ready(bytes) => {
                if let Some(Ok(bytes)) = &bytes {
                    *self_.byte_counter += bytes.len() as u64;
                    if let &mut Some(size) = self_.size {
                        if let Err(e) = self_.notifier.notify_status(ProgressInfo {
                            progress: *self_.byte_counter as f32 / size as f32,
                        }) {
                            warn!("Got an error while sending a notification, will not send further notifications: {:?}", e);

                            // prevent further attempts to send notification because it will be noisy otherwise
                            *self_.size = None;
                        }
                    }
                }

                Poll::Ready(bytes)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

async fn stream_url(
    client: &Client,
    url: Url,
    notifier: Notifier,
) -> anyhow::Result<(BoxStream<'static, futures::io::Result<Bytes>>, u64)> {
    let resp = client.execute(client.get(url).build()?).await?;

    let size = resp.content_length().context("No content length??")?;

    debug!("Streaming {:?} bytes...", size);

    let stream = resp.bytes_stream().map_err(|e| anyhow::Error::new(e));

    let stream = ProgressStream::new(stream, Some(size), notifier);

    let stream = stream
        .map_err(|e| futures::io::Error::new(ErrorKind::Other, e))
        .boxed();

    Ok((stream, size))
}
