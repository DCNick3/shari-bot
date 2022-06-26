use crate::dispatcher::DownloadDispatcher;
use crate::downloader::Downloader;
use anyhow::{anyhow, Context, Result};
use futures::{FutureExt, TryStreamExt};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use teloxide::{
    adaptors::AutoSend,
    dispatching::UpdateFilterExt,
    dptree,
    error_handlers::LoggingErrorHandler,
    payloads::EditMessageTextSetters,
    payloads::SendMessageSetters,
    payloads::SendVideoSetters,
    requests::Requester,
    types::ParseMode,
    types::{
        ChatId, ChatKind, InputFile, MediaKind, Message, MessageCommon, MessageEntityKind,
        MessageKind, Update,
    },
    utils::markdown,
    Bot,
};
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;
use tokio::{pin, select};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, error, info, warn};
use url::Url;

const SUPERUSER: ChatId = ChatId(379529027);

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

pub async fn run_bot(bot: AutoSend<Bot>, dispatcher: Arc<DownloadDispatcher>) {
    let handler = Update::filter_message().endpoint(handler);

    teloxide::dispatching::Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![dispatcher])
        // If no handler succeeded to handle an update, this closure will be called.
        .default_handler(|upd| async move {
            warn!("Unhandled update: {:?}", upd);
        })
        // If the dispatcher fails for some reason, execute this handler.
        .error_handler(LoggingErrorHandler::with_custom_text(
            "An error has occurred in the dispatcher",
        ))
        .build()
        .setup_ctrlc_handler()
        .dispatch()
        .await;
}

async fn upload_video(
    bot: AutoSend<Bot>,
    downloader: Arc<dyn Downloader>,
    url: Url,
    chat_id: ChatId,
    message_id: i32,
    notifier: Notifier,
) -> anyhow::Result<()> {
    let stream = downloader.download(url, notifier).await?;
    let stream = stream.into_async_read().compat();

    bot.send_video(chat_id, InputFile::read(stream).file_name("video.mp4"))
        .reply_to_message_id(message_id)
        .await?;

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
    bot: AutoSend<Bot>,
    dispatcher: Arc<DownloadDispatcher>,
) -> anyhow::Result<()> {
    let chat = message.chat;
    debug!("Got message from {:?}", chat.id);
    if !matches!(chat.kind, ChatKind::Private(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if chat.id != SUPERUSER {
        info!("Ignoring message from non-superuser ({:?})", chat);

        bot.send_message(
            chat.id,
            "sowwy i am not awwowed to spek with pepel i donbt now (yet) (/ω＼)",
        )
        .reply_to_message_id(message.id)
        .await?;
        return Ok(());
    }

    if let MessageKind::Common(MessageCommon {
        media_kind: MediaKind::Text(text),
        ..
    }) = message.kind.clone()
    {
        debug!("Text Message: {:#?}", text);

        if let Some(url) = text
            .entities
            .iter()
            .find(|e| e.kind == MessageEntityKind::Url)
        {
            let url = &text.text[url.offset..url.offset + url.length];
            let url = Url::parse(url).context("Parsing Url that teloxide thinks is a Url")?;

            debug!("Extracted URL: {}", url);

            if let Some(downloader) = dispatcher.find_downloader(&url) {
                debug!("Found downloader: {:?}", downloader);

                let status_message = bot
                    .send_message(chat.id, "Wowking~   (ﾉ>ω<)ﾉ")
                    .reply_to_message_id(message.id)
                    .await?;

                let (notifier, mut notification_rx) = Notifier::make();

                let upload_fut =
                    upload_video(bot.clone(), downloader, url, chat.id, message.id, notifier)
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

                            let message = format!("{}\n\n{}", markdown::escape(&message), markdown::code_block(&progressbar));

                            bot.edit_message_text(
                                chat.id,
                                status_message.id,
                                message,
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .await?;

                            magic_idx += 1;
                            if magic_idx == magic_parts.len() {
                                magic_idx = 0;
                            }
                        },
                        r = &mut upload_fut => {
                            break r;
                        }
                    }
                };

                match r {
                    Ok(_) => {
                        info!("Successfully sent video!");
                        bot.edit_message_text(
                            chat.id,
                            status_message.id,
                            "did it!1!1!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆",
                        )
                        .await?;
                    }
                    Err(e) => {
                        error!("Error occurred while sending the video: {:?}", e);
                        bot.edit_message_text(
                            chat.id,
                            status_message.id,
                            format!(
                                "{}\n\n{}",
                                markdown::escape("ewwow(((99  .･ﾟﾟ･(／ω＼)･ﾟﾟ･."),
                                markdown::code_block(&markdown::escape_code(&format!("{:?}", e)))
                            ),
                        )
                        .parse_mode(ParseMode::MarkdownV2)
                        .await?;
                    }
                }
            } else {
                bot.send_message(chat.id, "I donbt no ho to doload tis url((999")
                    .reply_to_message_id(message.id)
                    .await?;
            }
        } else {
            bot.send_message(
                chat.id,
                "Sen me smth with a URL in it and I wiww try to figuwe it out UwU",
            )
            .reply_to_message_id(message.id)
            .await?;
        }
    } else {
        bot.send_message(chat.id, "I donbt understan ☆⌒(> _ <)")
            .reply_to_message_id(message.id)
            .await?;
    }

    Ok(teloxide::respond(())?)
}
