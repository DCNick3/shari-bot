use crate::dispatcher::DownloadDispatcher;
use anyhow::{anyhow, Context, Result};
use std::sync::Arc;
use std::time::Duration;
use teloxide::adaptors::AutoSend;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::error_handlers::LoggingErrorHandler;
use teloxide::payloads::SendMessageSetters;
use teloxide::requests::{Requester, ResponseResult};
use teloxide::types::{
    ChatId, ChatKind, InputFile, MediaKind, Message, MessageCommon, MessageEntityKind, MessageKind,
    Update,
};
use teloxide::{dptree, Bot};
use tokio::sync::watch::Receiver;
use tokio::sync::watch::Sender;
use tracing::{debug, info, warn};

const SUPERUSER: ChatId = ChatId(379529027);

pub struct ProgressInfo {
    // TODO: probably add progressbar for multiple stages i dunno
    pub progress: u32,
    pub text: Arc<String>,
}

pub struct Notifier {
    chan: Sender<ProgressInfo>,
}

impl Notifier {
    fn make() -> (Self, Receiver<ProgressInfo>) {
        let (tx, rx) = tokio::sync::watch::channel(ProgressInfo {
            progress: 0,
            text: Arc::new("".to_string()),
        });

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

async fn handler(
    message: Message,
    bot: AutoSend<Bot>,
    dispatcher: Arc<DownloadDispatcher>,
) -> ResponseResult<()> {
    let chat = message.chat;
    debug!("Got message from {:?}", chat.id);
    if !matches!(chat.kind, ChatKind::Private(_)) {
        info!("Ignoring message not from private chat ({:?})", chat);
    }

    if chat.id != SUPERUSER {
        info!("Ignoring message from non-superuser ({:?})", chat);
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

            debug!("Extracted URL: {}", url);

            if let Some(downloader) = dispatcher.find_downloader(url) {
                debug!("Found downloader: {:?}", downloader);

                let status_message = bot
                    .send_message(chat.id, "Working  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆")
                    .reply_to_message_id(message.id)
                    .await?;

                let (notifier, notification_rx) = Notifier::make();

                tokio::time::sleep(Duration::from_secs(1)).await;

                bot.edit_message_text(
                    chat.id,
                    status_message.id,
                    "Working more!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆",
                )
                .await?;
            } else {
                bot.send_message(chat.id, "I donbt no ho to doload tis url((999")
                    .reply_to_message_id(message.id)
                    .await?;
            }
        } else {
            bot.send_message(
                chat.id,
                "Send me something with a URL in it and I'll try to figure it out UwU",
            )
            .reply_to_message_id(message.id)
            .await?;
        }
    } else {
        bot.send_message(chat.id, "I donbt understan ☆⌒(> _ <)")
            .reply_to_message_id(message.id)
            .await?;
    }

    teloxide::respond(())
}
