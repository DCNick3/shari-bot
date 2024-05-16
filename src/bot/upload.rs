use std::{sync::Arc, time::Duration};

use futures::{FutureExt, TryStreamExt};
use grammers_client::{
    types::{Attribute, Message},
    Client, InputMessage,
};
use snafu::{FromString, ResultExt, Snafu};
use tokio::{
    select,
    sync::watch::{Receiver, Sender},
    time::timeout,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, info_span, instrument, Instrument};
use url::Url;

use crate::{
    bot::{lang::Lang, markdown},
    downloader::{BytesStream, Downloader, VideoDownloadResult},
    whatever::Whatever,
};

#[derive(Debug, Snafu)]
pub enum UploadError {
    Timeout,
    Other { source: Whatever },
}

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

    let upload_fut = upload_video(client, downloader, url, initial_message, notifier)
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
        err = status_update_fut => return Err(err.unwrap_err()).context(OtherSnafu),
        r = upload_fut => {
            debug!("Upload future finished");
            match r {
                Ok(r) => return Ok(r.context(OtherSnafu)?),
                Err(_) => return Err(UploadError::Timeout),
            }
        }
    }
}

async fn upload_video(
    bot: &Client,
    downloader: Arc<dyn Downloader>,
    url: Url,
    initial_message: &Message,
    notifier: UploadNotifier,
) -> Result<(), Whatever> {
    let link_text = downloader.link_text();

    let VideoDownloadResult {
        canonical_url,
        video_information,
        video_stream: BytesStream { stream, size },
    } = downloader.download(url.clone(), notifier).await?;
    let mut stream = stream.into_async_read().compat();

    debug!("Uploading the stream to telegram...");
    let uploaded_video = bot
        .upload_stream(&mut stream, size as usize, "video.mp4".to_string())
        .await
        .whatever_context("Uploading video")?;

    debug!("Sending the video message...");
    let mut message = InputMessage::markdown(markdown::link(canonical_url.as_str(), &link_text))
        .document(uploaded_video);
    if let Some(video_information) = video_information {
        // big files require this information
        // short videos can be sent without it
        // we only return this from youtube videos for now
        message = message.attribute(Attribute::Video {
            h: video_information.height,
            w: video_information.width,
            duration: video_information.duration,
            round_message: false,
            supports_streaming: true,
        });
    }

    initial_message
        .reply(message)
        .await
        .whatever_context("Sending video message")?;

    debug!("Successfully sent video!");

    Ok(())
}

struct StatusMessageState {
    magic_index: usize,
    status_receiver: Receiver<UploadStatus>,
    previous_text: Option<String>,
}

impl StatusMessageState {
    const MAGIC_PARTS: &'static [&'static str] = &[":｡", "･:*", ":･ﾟ", "’★,｡", "･:*", ":･ﾟ", "’☆"];

    pub fn new(status_receiver: Receiver<UploadStatus>) -> Self {
        Self {
            magic_index: 1,
            status_receiver,
            previous_text: None,
        }
    }

    fn format_progress_bar(progress: f32) -> String {
        const PROGRESS_BAR_LENGTH: u32 = 30;

        let progress = (progress * PROGRESS_BAR_LENGTH as f32).round() as u32;

        let filled = (0..progress).map(|_| 'O').collect::<String>();
        let empty = (progress..PROGRESS_BAR_LENGTH)
            .map(|_| '.')
            .collect::<String>();

        let progressbar = format!("{}{}", filled, empty);

        progressbar
    }

    pub fn update(&mut self) -> Option<InputMessage> {
        self.magic_index += 1;
        if self.magic_index == Self::MAGIC_PARTS.len() {
            self.magic_index = 0;
        }

        let magic = &Self::MAGIC_PARTS[..self.magic_index];
        let magic = magic.join("");
        let message = format!("{} {}", Lang::StatusWorking, magic);

        let status = self.status_receiver.borrow_and_update();

        let body = match *status {
            UploadStatus::FetchingLink => Lang::StatusGettingLink.to_string(),
            UploadStatus::Uploading { progress } => {
                markdown::code_inline(&Self::format_progress_bar(progress))
            }
        };

        let message = format!("{}\n\n{}", message, body);
        if let Some(previous_text) = &self.previous_text {
            if previous_text == &message {
                return None;
            }
        }
        let result = InputMessage::markdown(&message);
        self.previous_text = Some(message);

        Some(result)
    }
}
