mod markdown;

use crate::dispatcher::DownloadDispatcher;
use crate::downloader::Downloader;
use anyhow::{anyhow, Context, Result};
use futures::{FutureExt, TryStreamExt};
use grammers_client::{
    button, reply_markup,
    types::{Chat, Media, Message},
    Client, InputMessage, Update,
};
use grammers_session::PackedChat;
use grammers_tl_types::enums;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;
use tokio::{pin, select};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, error, info, warn};
use url::Url;

const SUPERUSER: i64 = 379529027;

#[derive(Clone)]
pub struct ProgressInfo {
    // TODO: probably add progressbar for multiple stages i dunno
    pub progress: f32,
}

pub struct Notifier {
    chan: Sender<ProgressInfo>,
}

impl Notifier {
    fn make() -> (Self, Receiver<ProgressInfo>) {
        let (tx, rx) = tokio::sync::watch::channel(ProgressInfo { progress: 0.0 });

        (Self { chan: tx }, rx)
    }

    pub fn notify_status(&self, status: ProgressInfo) -> Result<()> {
        self.chan
            .send(status)
            .map_err(|_| anyhow!("Notification channel closed??"))
    }
}

pub async fn run_bot(bot: Client, dispatcher: Arc<DownloadDispatcher>) -> Result<()> {
    while let Some(update) = bot.next_update().await.context("Getting next update")? {
        let Update::NewMessage(message) = update else {
            continue;
        };
        if message.outgoing() {
            continue;
        }

        match handler(message, &bot, dispatcher.clone()).await {
            Ok(_) => {}
            Err(e) => {
                error!("Error occurred while handling message: {:?}", e);
            }
        }
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
    chat_id: PackedChat,
    message_id: i32,
    notifier: Notifier,
) -> anyhow::Result<()> {
    let link_text = downloader.link_text();

    let (stream, size) = downloader.download(url.clone(), notifier).await?;
    let mut stream = stream.into_async_read().compat();

    debug!("Uploading the stream to telegram...");
    let uploaded_video = bot
        .upload_stream(&mut stream, size as usize, "video.mp4".to_string())
        .await
        .context("Uploading video")?;

    debug!("Sending the video message...");
    bot.send_message(
        chat_id,
        InputMessage::text("")
            .reply_to(Some(message_id))
            .document(uploaded_video)
            .reply_markup(&make_keyboard(link_text, url)),
    )
    .await
    .context("Sending video message")?;

    debug!("Successfully sent video!");

    // bot.send_video(chat_id, InputFile::read(stream).file_name("video.mp4"))
    //     .reply_to_message_id(message_id)
    //     .reply_markup(ReplyMarkup::InlineKeyboard(make_keyboard(link_text, url)))
    //     .await?;

    Ok(())
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

async fn handler(
    message: Message,
    bot: &Client,
    dispatcher: Arc<DownloadDispatcher>,
) -> Result<()> {
    let chat = message.chat();
    debug!("Got message from {:?}", chat.id());
    if !matches!(chat, Chat::User(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if chat.id() != SUPERUSER {
        info!("Ignoring message from non-superuser ({:?})", chat);

        message
            .reply("sowwy i am not awwowed to spek with pepel i donbt now (yet) (/ω＼)")
            .await?;

        return Ok(());
    }

    if message
        .media()
        .map_or(false, |m| matches!(m, Media::WebPage(_)))
    {
        let text = message.text();
        debug!("Text Message: {:#?}", text);

        let text = text.encode_utf16().collect::<Vec<_>>();

        if let Some(url) = message
            .fmt_entities()
            .into_iter()
            .flatten()
            .find_map(|e| match e {
                enums::MessageEntity::Url(url) => Some(url),
                _ => None,
            })
        {
            let url = &text[url.offset as usize..(url.offset + url.length) as usize];
            let url = String::from_utf16(url).context("Parsing Url from message")?;
            let url = Url::parse(&url).context("Parsing Url that telegram marked as a Url")?;

            debug!("Extracted URL: {}", url);

            if let Some(downloader) = dispatcher.find_downloader(&url) {
                debug!("Found downloader: {:?}", downloader);

                let mut current_status_message_text = "Wowking~   (ﾉ>ω<)ﾉ".to_string();
                let status_message = message.reply(current_status_message_text.as_str()).await?;

                let (notifier, mut notification_rx) = Notifier::make();

                let upload_fut = upload_video(
                    &bot,
                    downloader,
                    url,
                    chat.clone().into(),
                    message.id(),
                    notifier,
                )
                .fuse();

                let mut interval = tokio::time::interval(Duration::from_secs(1));

                pin!(upload_fut);

                let magic_parts = [":｡", "･:*", ":･ﾟ", "’★,｡", "･:*", ":･ﾟ", "’☆"];
                let mut magic_idx: usize = 1;

                let r = loop {
                    select! {
                        _ = interval.tick() => {
                            let magic = &magic_parts[..magic_idx];
                            let magic = magic.join("");
                            let message = format!("Wowking~   (ﾉ>ω<)ﾉ {}", magic);

                            // it's important to clone here so that the borrow does not live up to suspension point
                            let status: ProgressInfo = notification_rx.borrow_and_update().deref().clone();

                            let progressbar = format_progress_bar(status.progress);

                            let message_text = format!("{}\n\n{}", markdown::escape(
                                &message
                                ), markdown::code_inline(&progressbar));
                            let message = InputMessage::markdown(&message_text);

                            debug!("Updating status message");

                            if message_text != current_status_message_text {
                                bot.edit_message(
                                    chat.clone(),
                                    status_message.id(),
                                    message,
                                )
                                .await?;
                                current_status_message_text = message_text;
                            }

                            magic_idx += 1;
                            if magic_idx == magic_parts.len() {
                                magic_idx = 0;
                            }
                        },
                        r = &mut upload_fut => {
                            debug!("Upload future finished");
                            break r;
                        }
                    }
                };

                match r {
                    Ok(_) => {
                        info!("Successfully sent video!");
                        bot.edit_message(
                            chat,
                            status_message.id(),
                            "did it!1!1!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆",
                        )
                        .await?;
                    }
                    Err(e) => {
                        error!("Error occurred while sending the video: {:?}", e);
                        bot.edit_message(
                            chat,
                            status_message.id(),
                            InputMessage::markdown(format!(
                                "{}\n\n{}",
                                markdown::escape("ewwow(((99  .･ﾟﾟ･(／ω＼)･ﾟﾟ･."),
                                markdown::code_block(&markdown::escape_code(&format!("{:?}", e)))
                            )),
                        )
                        .await?;
                    }
                }
            } else {
                bot.send_message(
                    chat,
                    InputMessage::text("I donbt no ho to doload tis url((999")
                        .reply_to(Some(message.id())),
                )
                .await?;
            }
        } else {
            bot.send_message(
                chat,
                InputMessage::text(
                    "Sen me smth with a URL in it and I wiww try to figuwe it out UwU",
                )
                .reply_to(Some(message.id())),
            )
            .await?;
        }
    } else {
        bot.send_message(
            chat,
            InputMessage::text("I donbt understan ☆⌒(> _ <)").reply_to(Some(message.id())),
        )
        .await?;
    }

    Ok(())
}
