mod commands;
mod markdown;
pub mod whitelist;

use crate::{
    bot::commands::handle_command, dispatcher::DownloadDispatcher, downloader::Downloader,
    whatever::Whatever,
};
use futures::{FutureExt, TryStreamExt};
use grammers_client::{
    button, reply_markup,
    types::{Attribute, Chat, Message},
    Client, InputMessage, Update,
};
use grammers_tl_types::enums;
use indoc::indoc;
use serde::{Deserialize, Serialize};
use snafu::{FromString, ResultExt, Snafu};
use std::{borrow::Cow, collections::HashSet, sync::Arc, time::Duration};
use tokio::{
    select,
    sync::{
        watch::{Receiver, Sender},
        Mutex,
    },
    time::timeout,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};
use url::Url;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Ord, PartialOrd, Copy, Clone, Hash)]
pub struct UserId(pub i64);

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

    pub fn notify_status(&self, status: UploadStatus) -> Result<(), Whatever> {
        self.chan
            .send(status)
            .map_err(|_| Whatever::without_source("Notification channel closed??".to_owned()))
    }
}

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
    notifier: Notifier,
) -> Result<(), Whatever> {
    let link_text = downloader.link_text();

    let (video_information, stream, size) = downloader.download(url.clone(), notifier).await?;
    let mut stream = stream.into_async_read().compat();

    debug!("Uploading the stream to telegram...");
    let uploaded_video = bot
        .upload_stream(&mut stream, size as usize, "video.mp4".to_string())
        .await
        .whatever_context("Uploading video")?;

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

#[instrument(skip_all, fields(url = %url, downloader_name = downloader.link_text()))]
async fn upload_with_status_updates(
    client: &Client,
    initial_message: &Message,
    status_message: &Message,
    url: Url,
    downloader: Arc<dyn Downloader>,
    video_handling_timeout: Duration,
) -> Result<(), UploadError> {
    let (notifier, notification_rx) = Notifier::make();

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

#[instrument(skip_all, fields(chat_id = message.chat().id(), username = message.chat().username()), err)]
async fn handle_message(
    message: Message,
    client: Client,
    dispatcher: Arc<DownloadDispatcher>,
    whitelist: Arc<Mutex<whitelist::Whitelist>>,
    superusers: Arc<HashSet<UserId>>,
    video_handling_timeout: Duration,
) -> Result<(), Whatever> {
    let chat = message.chat();
    debug!("Got message from {:?}", chat.id());
    if !matches!(chat, Chat::User(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if !superusers.contains(&UserId(chat.id()))
        && !whitelist.lock().await.contains(UserId(chat.id()))
    {
        info!("Ignoring message from non-superuser ({:?})", chat);

        message
            .reply("sowwy i am not awwowed to spek with pepel i donbt now (yet) (/ω＼)")
            .await
            .whatever_context("Sending reply")?;

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

    if superusers.contains(&UserId(chat.id())) {
        if let Some(command) = find_message_entity(&message, |e| match e {
            enums::MessageEntity::BotCommand(command) => Some(command),
            _ => None,
        }) {
            debug!("Found command");
            handle_command(&client, command, &message, whitelist).await?;
            return Ok(());
        } else {
            debug!("No commands were found");
        };
    }

    let Some(url) = find_message_entity(&message, |e| match e {
        enums::MessageEntity::Url(url) => Some(url),
        _ => None,
    }) else {
        message
            .reply(InputMessage::text(
                "Sen me smth with a URL in it and I wiww try to figuwe it out UwU",
            ))
            .await
            .whatever_context("Sending reply")?;
        return Ok(());
    };

    let url = &text[url.offset as usize..(url.offset + url.length) as usize];
    let url = String::from_utf16(url).whatever_context("Parsing Url from message")?;
    let url = Url::parse(&url).whatever_context("Parsing Url that telegram marked as a Url")?;

    debug!("Extracted URL: {}", url);

    let Some(downloader) = dispatcher.find_downloader(&url) else {
        message
            .reply("I donbt no ho to doload tis url((999")
            .await
            .whatever_context("Sending reply")?;
        return Ok(());
    };

    debug!("Found downloader: {:?}", downloader);

    let status_message = message
        .reply("Wowking~   (ﾉ>ω<)ﾉ")
        .await
        .whatever_context("Sending reply")?;

    let end_message = match upload_with_status_updates(
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
            Cow::Borrowed("did it!1!1!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆")
        }
        Err(UploadError::Timeout) => {
            warn!("Took too long to handle a message, stopped video handling");
            Cow::Borrowed(indoc!(
                r#"Took too long to download & upload the video, maybe the file is
                too large or the bot is under heavy load."#,
            ))
        }
        Err(UploadError::Other { inner: e }) => {
            error!("Error occurred while sending the video: {:?}", e);
            // TODO: make the error a code block
            // the markdown parser seems a bit buggy, so can't really use it here.
            Cow::Owned(format!("ewwow(((99  .･ﾟﾟ･(／ω＼)･ﾟﾟ･.\n\n{:?}", e))
        }
    };

    status_message
        .edit(end_message.as_ref())
        .await
        .whatever_context("Editing message")?;

    Ok(())
}
