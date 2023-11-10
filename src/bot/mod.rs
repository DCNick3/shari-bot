mod commands;
mod markdown;
pub mod whitelist;

use crate::bot::commands::handle_command;
use crate::dispatcher::DownloadDispatcher;
use crate::downloader::Downloader;
use anyhow::{anyhow, Context, Result};
use futures::{FutureExt, TryStreamExt};
use grammers_client::types::Attribute;
use grammers_client::{
    button, reply_markup,
    types::{Chat, Message},
    Client, InputMessage, Update,
};
use grammers_tl_types::enums;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};
use url::Url;

type UserId = i64;

const SUPERUSER: UserId = 379529027;

#[derive(Clone)]
pub enum UploadStatus {
    FetchingLink,
    Uploading { progress: f32 },
}

pub struct Notifier {
    chan: Sender<UploadStatus>,
}

impl Notifier {
    fn make() -> (Self, Receiver<UploadStatus>) {
        let (tx, rx) = tokio::sync::watch::channel(UploadStatus::FetchingLink);

        (Self { chan: tx }, rx)
    }

    pub fn notify_status(&self, status: UploadStatus) -> Result<()> {
        self.chan
            .send(status)
            .map_err(|_| anyhow!("Notification channel closed??"))
    }
}

pub async fn run_bot(
    client: &Client,
    dispatcher: Arc<DownloadDispatcher>,
    message_handle_timeout: Duration,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
) -> Result<()> {
    while let Some(update) = client.next_update().await.context("Getting next update")? {
        let Update::NewMessage(message) = update else {
            continue;
        };
        if message.outgoing() {
            continue;
        }

        let dispatcher = dispatcher.clone();
        let client = client.clone();
        let whitelist = whitelist.clone();
        tokio::spawn(async move {
            let task = timeout(
                message_handle_timeout,
                handle_message(message, client, dispatcher, whitelist),
            );

            if let Ok(handle_result) = task.await {
                match handle_result {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error occurred while handling message: {:?}", e);
                    }
                }
            } else {
                warn!("Took too long to handle a message, cancelled the task");
            }
        });
    }

    info!("Stopped getting updates!");
    Ok(())
}

fn make_keyboard(link_text: &str, url: Url) -> reply_markup::Inline {
    reply_markup::inline(vec![vec![button::url(link_text, url)]])
}

async fn upload_video(
    bot: &Client,
    downloader: Arc<dyn Downloader>,
    url: Url,
    initial_message: &Message,
    notifier: Notifier,
) -> Result<()> {
    let link_text = downloader.link_text();

    let (video_information, stream, size) = downloader.download(url.clone(), notifier).await?;
    let mut stream = stream.into_async_read().compat();

    debug!("Uploading the stream to telegram...");
    let uploaded_video = bot
        .upload_stream(&mut stream, size as usize, "video.mp4".to_string())
        .await
        .context("Uploading video")?;

    debug!("Sending the video message...");
    let mut message = InputMessage::text("")
        .document(uploaded_video)
        .reply_markup(&make_keyboard(link_text, url));
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
        .context("Sending video message")?;

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
        let message = format!("Wowking~   (ﾉ>ω<)ﾉ {}", magic);

        let status = self.status_receiver.borrow_and_update();

        let body = match *status {
            UploadStatus::FetchingLink => "Gettinb vid linkie (；⌣̀_⌣́)～".to_string(),
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

#[instrument(skip_all, fields(url = %url, downloader_name = downloader.link_text()))]
async fn upload_with_status_updates(
    client: &Client,
    initial_message: &Message,
    status_message: &Message,
    url: Url,
    downloader: Arc<dyn Downloader>,
) -> Result<()> {
    let (notifier, notification_rx) = Notifier::make();

    let upload_fut = upload_video(&client, downloader, url, initial_message, notifier)
        .fuse()
        .instrument(info_span!("upload_video"));

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
                    .context("Editing status message")?;
            }
        }

        // unreachable, but not useless
        // it drives the type inference for the async block
        #[allow(unreachable_code)]
        Ok::<(), anyhow::Error>(())
    }
    .instrument(info_span!("update_status_message"))
    .fuse();

    select! {
        err = status_update_fut => return Err(err.unwrap_err()),
        r = upload_fut => {
            debug!("Upload future finished");
            r?;
        }
    }

    Ok(())
}

fn find_message_entity<E, F>(message: &Message, finder: F) -> Option<&E>
where
    F: for<'a> FnMut(&'a enums::MessageEntity) -> Option<&'a E>,
{
    message
        .fmt_entities()
        .into_iter()
        .flatten()
        .find_map(finder)
}

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()))]
async fn handle_message(
    message: Message,
    client: Client,
    dispatcher: Arc<DownloadDispatcher>,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
) -> Result<()> {
    let chat = message.chat();
    debug!("Got message from {:?}", chat.id());
    if !matches!(chat, Chat::User(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if chat.id() != SUPERUSER && !whitelist.lock().await.contains(chat.id()) {
        info!("Ignoring message from non-superuser ({:?})", chat);

        message
            .reply("sowwy i am not awwowed to spek with pepel i donbt now (yet) (/ω＼)")
            .await?;

        return Ok(());
    }

    // if !message
    //     .media()
    //     .map_or(false, |m| matches!(m, Media::WebPage(_)))
    // {
    //     message
    //         .reply(InputMessage::text("I donbt understan ☆⌒(> _ <)"))
    //         .await?;
    // }

    let text = message.text();
    debug!("Text Message: {:#?}", text);

    let text = text.encode_utf16().collect::<Vec<_>>();

    if let Some(command) = find_message_entity(&message, |e| match e {
        enums::MessageEntity::BotCommand(command) => Some(command),
        _ => None,
    }) {
        handle_command(command, &message, whitelist).await?;
    } else {
        debug!("No commands were found");
    };

    let Some(url) = find_message_entity(&message, |e| match e {
        enums::MessageEntity::Url(url) => Some(url),
        _ => None,
    }) else {
        message
            .reply(InputMessage::text(
                "Sen me smth with a URL in it and I wiww try to figuwe it out UwU",
            ))
            .await?;
        return Ok(());
    };

    let url = &text[url.offset as usize..(url.offset + url.length) as usize];
    let url = String::from_utf16(url).context("Parsing Url from message")?;
    let url = Url::parse(&url).context("Parsing Url that telegram marked as a Url")?;

    debug!("Extracted URL: {}", url);

    let Some(downloader) = dispatcher.find_downloader(&url) else {
        message
            .reply("I donbt no ho to doload tis url((999")
            .await?;
        return Ok(());
    };

    debug!("Found downloader: {:?}", downloader);

    let status_message = message.reply("Wowking~   (ﾉ>ω<)ﾉ").await?;

    let end_message =
        match upload_with_status_updates(&client, &message, &status_message, url, downloader).await
        {
            Ok(_) => {
                info!("Successfully sent video!");
                Cow::Borrowed("did it!1!1!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆")
            }
            Err(e) => {
                error!("Error occurred while sending the video: {:?}", e);
                // TODO: make the error a code block
                // the markdown parser seems a bit buggy, so can't really use it here.
                Cow::Owned(format!("ewwow(((99  .･ﾟﾟ･(／ω＼)･ﾟﾟ･.\n\n{:?}", e))
            }
        };

    status_message.edit(end_message.as_ref()).await?;

    Ok(())
}
