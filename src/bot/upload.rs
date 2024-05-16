use crate::bot;
use crate::bot::{StatusMessageState, UploadError};
use crate::downloader::Downloader;
use crate::whatever::Whatever;
use futures::FutureExt;
use grammers_client::types::Message;
use grammers_client::Client;
use snafu::{FromString, ResultExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::watch::{Receiver, Sender};
use tokio::time::timeout;
use tracing::{debug, info_span, instrument, Instrument};
use url::Url;

#[derive(Clone)]
pub enum UploadStatus {
    FetchingLink,
    Uploading { progress: f32 },
}

pub struct UploadNotifier {
    chan: Sender<UploadStatus>,
}

impl UploadNotifier {
    fn make() -> (Self, Receiver<UploadStatus>) {
        let (tx, rx) = tokio::sync::watch::channel(UploadStatus::FetchingLink);

        (Self { chan: tx }, rx)
    }

    pub fn notify_status(&self, status: UploadStatus) -> Result<(), Whatever> {
        self.chan
            .send(status)
            .map_err(|_| Whatever::without_source("Notification channel closed??".to_owned()))
    }
}

#[instrument(skip_all, fields(url = %url, downloader_name = downloader.link_text()))]
pub async fn upload_with_status_updates(
    client: &Client,
    initial_message: &Message,
    status_message: &Message,
    url: Url,
    downloader: Arc<dyn Downloader>,
    video_handling_timeout: Duration,
) -> Result<(), UploadError> {
    let (notifier, notification_rx) = UploadNotifier::make();

    let upload_fut = bot::upload_video(client, downloader, url, initial_message, notifier)
        .fuse()
        .instrument(info_span!("upload_video"));
    let upload_fut = timeout(video_handling_timeout, upload_fut);

    let mut interval = tokio::time::interval(Duration::from_secs(1));

    let status_update_fut = async {
        let mut magic = StatusMessageState::new(notification_rx);

        loop {
            interval.tick().await;
            if let Some(message) = magic.update() {
                debug!("Updating status message");
                status_message
                    .edit(message)
                    .await
                    .whatever_context("Editing status message")?;
            }
        }

        // unreachable, but not useless
        // it drives the type inference for the async block
        #[allow(unreachable_code)]
        Ok::<(), Whatever>(())
    }
    .instrument(info_span!("update_status_message"))
    .fuse();
    select! {
        err = status_update_fut => return Err(err.unwrap_err().into()),
        r = upload_fut => {
            debug!("Upload future finished");
            match r {
                Ok(r) => return Ok(r?),
                Err(_) => return Err(UploadError::Timeout),
            }
        }
    }
}
