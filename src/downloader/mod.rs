pub mod tiktok;
pub mod youtube;

use crate::bot::{Notifier, UploadStatus};
use crate::whatever::Whatever;
use crate::{StreamExt, TryStreamExt};
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::Stream;
use pin_project_lite::pin_project;
use reqwest::Client;
use snafu::{OptionExt, ResultExt};
use std::fmt::Debug;
use std::io::ErrorKind;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tracing::{debug, warn};
use url::Url;

pub struct VideoInformation {
    pub width: i32,
    pub height: i32,
    pub duration: Duration,
}

pub struct BytesStream {
    pub stream: BoxStream<'static, futures::io::Result<Bytes>>,
    pub size: u64,
}

pub struct VideoDownloadResult {
    pub canonical_url: Url,
    pub video_information: Option<VideoInformation>,
    pub video_stream: BytesStream,
}

#[async_trait]
pub trait Downloader: Debug + Send + Sync {
    fn probe_url(&self, url: &Url) -> bool;
    fn link_text(&self) -> &'static str;
    async fn download(
        self: Arc<Self>,
        url: Url,
        notifier: Notifier,
    ) -> Result<VideoDownloadResult, Whatever>;
}

pin_project! {
    struct ProgressStream<T: Stream<Item = Result<Bytes, reqwest::Error>>> {
        #[pin]
        stream: T,
        size: Option<u64>,
        byte_counter: u64,
        notifier: Notifier,
    }
}

impl<T: Stream<Item = Result<Bytes, reqwest::Error>>> ProgressStream<T> {
    pub fn new(stream: T, size: Option<u64>, notifier: Notifier) -> Self {
        Self {
            stream,
            size,
            byte_counter: 0,
            notifier,
        }
    }
}

impl<T: Stream<Item = Result<Bytes, reqwest::Error>>> Stream for ProgressStream<T> {
    type Item = Result<Bytes, reqwest::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_ = self.project();
        match self_.stream.poll_next(cx) {
            Poll::Ready(bytes) => {
                if let Some(Ok(bytes)) = &bytes {
                    *self_.byte_counter += bytes.len() as u64;
                    if let &mut Some(size) = self_.size {
                        if let Err(e) = self_.notifier.notify_status(UploadStatus::Uploading {
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
) -> Result<BytesStream, Whatever> {
    let resp = client
        .execute(
            client
                .get(url)
                .build()
                .whatever_context("Building a request")?,
        )
        .await
        .whatever_context("Executing the request")?;

    let size = resp
        .content_length()
        .whatever_context("No content length??")?;

    debug!("Streaming {:?} bytes...", size);

    let stream = resp.bytes_stream();

    let stream = ProgressStream::new(stream, Some(size), notifier);

    let stream = stream
        .map_err(|e| futures::io::Error::new(ErrorKind::Other, e))
        .boxed();

    let stream = BytesStream { stream, size };

    Ok(stream)
}
