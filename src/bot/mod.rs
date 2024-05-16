mod commands;
mod lang;
mod markdown;
mod upload;
pub mod whitelist;

pub use self::upload::UploadNotifier;
pub use self::upload::UploadStatus;
use crate::bot::lang::Lang;
use crate::downloader::{BytesStream, VideoDownloadResult};
use crate::{
    bot::commands::handle_command, dispatcher::DownloadDispatcher, downloader::Downloader,
    whatever::Whatever,
};
use futures::TryStreamExt as _;
use grammers_client::{
    button, reply_markup,
    types::{Attribute, Chat, Message},
    Client, InputMessage, Update,
};
use grammers_tl_types::enums;
use serde::{Deserialize, Serialize};
use snafu::{ResultExt as _, Snafu};
use std::{collections::HashSet, sync::Arc, time::Duration};
use tokio::sync::{watch::Receiver, Mutex};
use tokio_util::compat::FuturesAsyncReadCompatExt as _;
use tracing::{debug, error, info, instrument, warn};
use url::Url;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd, Copy, Clone, Hash)]
pub struct UserId(pub i64);

pub async fn run_bot(
    client: &Client,
    dispatcher: Arc<DownloadDispatcher>,
    video_handling_timeout: Duration,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
    superusers: HashSet<UserId>,
) -> Result<(), Whatever> {
    let superusers = Arc::new(superusers);
    while let Some(update) = client
        .next_update()
        .await
        .whatever_context("Getting next update")?
    {
        let Update::NewMessage(message) = update else {
            continue;
        };
        if message.outgoing() {
            continue;
        }

        let dispatcher = dispatcher.clone();
        let client = client.clone();
        let whitelist = whitelist.clone();
        let superusers = superusers.clone();
        tokio::spawn(async move {
            // error are logged by tracing instrument macro
            let _ = handle_message(
                message,
                client,
                dispatcher,
                whitelist,
                superusers,
                video_handling_timeout,
            )
            .await;
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
    let mut message = InputMessage::text("")
        .document(uploaded_video)
        .reply_markup(&make_keyboard(link_text, canonical_url));
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

#[derive(Debug, Snafu)]
enum UploadError {
    Timeout,
    Other { inner: Whatever },
}

impl From<Whatever> for UploadError {
    fn from(value: Whatever) -> Self {
        UploadError::Other { inner: value }
    }
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

pub enum MessageResult {
    Reply(InputMessage),
    Ignore,
}

fn reply(message: impl Into<InputMessage>) -> Result<MessageResult, Whatever> {
    Ok(MessageResult::Reply(message.into()))
}

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()))]
async fn handle_message_impl(
    message: &Message,
    client: Client,
    dispatcher: Arc<DownloadDispatcher>,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
    superusers: Arc<HashSet<UserId>>,
    video_handling_timeout: Duration,
) -> Result<MessageResult, Whatever> {
    let chat = message.chat();
    debug!("Got message from {:?}", chat.id());
    if !matches!(chat, Chat::User(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if !superusers.contains(&UserId(chat.id()))
        && !whitelist.lock().await.contains(&UserId(chat.id()))
    {
        info!("Ignoring message from non-superuser ({:?})", chat);

        return reply(Lang::NoAccess);
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

    // commands are only for superusers
    if superusers.contains(&UserId(chat.id())) {
        if let Some(command) = find_message_entity(&message, |e| match e {
            enums::MessageEntity::BotCommand(command) => Some(command),
            _ => None,
        }) {
            debug!("Found command");
            return reply(handle_command(&client, command, &message, whitelist).await?);
        } else {
            debug!("No commands were found");
        };
    }

    let Some(url) = find_message_entity(&message, |e| match e {
        enums::MessageEntity::Url(url) => Some(url),
        _ => None,
    }) else {
        return reply(Lang::NoUrl);
    };

    // extract the url entity text
    let url = &text[url.offset as usize..(url.offset + url.length) as usize];
    let url = String::from_utf16(url).whatever_context("Parsing Url codepoints as string")?;
    let url = Url::parse(&url).whatever_context("Parsing Url that telegram marked as a Url")?;

    debug!("Extracted URL: {}", url);

    let Some(downloader) = dispatcher.find_downloader(&url) else {
        return reply(Lang::UnsupportedUrl);
    };

    debug!("Found downloader: {:?}", downloader);

    let status_message = message
        .reply(Lang::StatusWorking)
        .await
        .whatever_context("Sending reply")?;

    let end_message = match upload::upload_with_status_updates(
        &client,
        &message,
        &status_message,
        url,
        downloader,
        video_handling_timeout,
    )
    .await
    {
        Ok(_) => {
            info!("Successfully sent video!");
            Lang::ResultSuccess
        }
        Err(UploadError::Timeout) => {
            warn!("Took too long to handle a message, stopped video handling");
            Lang::ResultErrorTimeout
        }
        Err(UploadError::Other { inner: e }) => {
            error!("Error occurred while sending the video: {:?}", e);
            return Err(e);
        }
    };

    status_message
        .edit(end_message)
        .await
        .whatever_context("Editing message")?;

    Ok(MessageResult::Ignore)
}

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()), err(Debug))]
async fn handle_message(
    message: Message,
    client: Client,
    dispatcher: Arc<DownloadDispatcher>,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
    superusers: Arc<HashSet<UserId>>,
    video_handling_timeout: Duration,
) -> Result<(), Whatever> {
    let result = handle_message_impl(
        &message,
        client,
        dispatcher,
        whitelist,
        superusers,
        video_handling_timeout,
    )
    .await;

    // reply to the user if there's an error or the handler requested a reply
    // any error here will only be reported to the tracing, not to the user (because sending a message after a failed message will probably fail too..)
    match result {
        Ok(MessageResult::Reply(reply)) => {
            message
                .reply(reply)
                .await
                .whatever_context("Replying to the message")?;
        }
        Ok(MessageResult::Ignore) => {}
        Err(e) => {
            let report = snafu::Report::from_error(e).to_string();
            error!("Error while handing a message: {}", report);

            // TODO: make the error a code block
            // the markdown parser seems a bit buggy, so can't really use it here.
            // TODO: and now that a Lang is here, it's even less clear as to how
            message
                .reply(Lang::ResultGenericError(report))
                .await
                .whatever_context("Sending the error message to the user")?;
        }
    };

    Ok(())
}
