use crate::bot::markdown;
use anyhow::Context;
use grammers_client::client::auth::InvocationError;
use grammers_client::types::{Chat, Message, User};
use grammers_client::{Client, InputMessage};
use grammers_tl_types::enums::MessageEntity;
use grammers_tl_types::types::MessageEntityBotCommand;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

#[derive(Debug)]
struct Alias(String);

impl Alias {
    /// If `None`, then the argument is not a user mention
    fn parse(message: &Message, arg: &str) -> Option<Self> {
        let text = message.text();
        let text = text.encode_utf16().collect::<Vec<_>>();
        let mentions: Vec<_> = message
            .fmt_entities()
            .into_iter()
            .flatten()
            .filter_map(|e| match e {
                MessageEntity::Mention(m) => Some(m),
                _ => None,
            })
            .collect();
        let result = mentions.iter().find_map(|mention| {
            let name = &text[mention.offset as usize..(mention.offset + mention.length) as usize];
            let Ok(name) = String::from_utf16(name) else {
                warn!("Could not parse mention text form {:?}", name);
                return None;
            };
            if name == arg {
                Some(Alias(name.trim_start_matches('@').to_string()))
            } else {
                None
            }
        });
        if result.is_none() {
            debug!("Did not match '{arg}' with any aliases {mentions:?}");
        };
        result
    }

    async fn resolve(&self, client: &Client) -> anyhow::Result<Option<User>> {
        let id = match client.resolve_username(&self.0).await {
            Ok(id) => id,
            Err(InvocationError::Rpc(r)) if r.code == 400 => None,
            Err(e) => return Err(e).context("Requesting username resolve"),
        };
        let user_id = id.map(|chat| match chat {
            Chat::User(id) => Some(id),
            Chat::Group(_) => None,
            Chat::Channel(_) => None,
        });
        Ok(user_id.flatten())
    }
}

enum SuperuserCommand {
    WhitelistInsert(Alias),
    WhitelistRemove(Alias),
    WhitelistGet,
}

#[derive(Debug)]
enum CommandParseError {
    UnknownCommand,
    NoArgumentsProvided,
    IncorrectArguments,
    ParsingError(anyhow::Error),
}

impl From<anyhow::Error> for CommandParseError {
    fn from(value: anyhow::Error) -> Self {
        Self::ParsingError(value)
    }
}

impl SuperuserCommand {
    fn parse(
        command: &MessageEntityBotCommand,
        message: &Message,
    ) -> Result<Self, CommandParseError> {
        let text = message.text();
        let text = text.encode_utf16().collect::<Vec<_>>();
        let command_text =
            &text[command.offset as usize..(command.offset + command.length) as usize];
        let command_text =
            String::from_utf16(command_text).context("Parsing command from message")?;
        let args = &text[(command.offset + command.length) as usize..];
        let args =
            String::from_utf16(args).context("Parsing arguments for the command from message")?;
        let mut args = args.split_whitespace();
        // expect??
        let command = command_text
            .split('@')
            .next()
            .context("Split produced no elements for some reason")?;

        match command {
            "/whitelist" | "/whitelist_get" => Ok(Self::WhitelistGet),
            "/whitelist_add" => {
                let arg = args.next().ok_or(CommandParseError::NoArgumentsProvided)?;
                let alias =
                    Alias::parse(message, arg).ok_or(CommandParseError::IncorrectArguments)?;
                Ok(Self::WhitelistInsert(alias))
            }
            "/whitelist_remove" => {
                let arg = args.next().ok_or(CommandParseError::NoArgumentsProvided)?;
                let alias =
                    Alias::parse(message, arg).ok_or(CommandParseError::IncorrectArguments)?;
                Ok(Self::WhitelistRemove(alias))
            }
            _ => Err(CommandParseError::UnknownCommand),
        }
    }
}

pub async fn handle_command(
    client: &Client,
    command: &MessageEntityBotCommand,
    message: &Message,
    whitelist: Arc<Mutex<crate::bot::whitelist::Whitelist>>,
) -> anyhow::Result<()> {
    let command = match SuperuserCommand::parse(command, message) {
        Ok(c) => c,
        Err(CommandParseError::UnknownCommand) => {
            message
                .reply(InputMessage::text(
                    "I don't know such command, /help might help",
                ))
                .await?;
            return Ok(());
        }
        Err(CommandParseError::NoArgumentsProvided) => {
            message
                .reply(InputMessage::text("Missing argument(-s), /help might help"))
                .await?;
            return Ok(());
        }
        Err(CommandParseError::IncorrectArguments) => {
            message
                .reply(InputMessage::text(
                    "I expected other arguments, /help might help",
                ))
                .await?;
            return Ok(());
        }
        Err(CommandParseError::ParsingError(e)) => return Err(e.context("Parsing command")),
    };

    match command {
        SuperuserCommand::WhitelistInsert(alias) => {
            let Some(user) = alias.resolve(client).await? else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                message
                    .reply(InputMessage::text(
                        "All telegram doesn't know this person 🔍\nDid you type the name correctly?",
                    ))
                    .await?;
                return Ok(());
            };
            info!(
                "Adding into whitelist user (name: {:?}, id: {})",
                user.username(),
                user.id()
            );
            let added = whitelist.lock().await.insert(user.id()).await?;
            if added {
                message
                    .reply(InputMessage::text(
                        "Added the user successfully! ✨ Now they can use this bot ✨",
                    ))
                    .await?;
            } else {
                message
                    .reply(InputMessage::text(
                        "✨ I already know this person! (or bot 🤔) ✨",
                    ))
                    .await?;
            }
        }
        SuperuserCommand::WhitelistRemove(alias) => {
            let Some(user) = alias.resolve(client).await? else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                message
                    .reply(InputMessage::text(
                        "All telegram doesn't know this person 🔍\nDid you type the name correctly?",
                    ))
                    .await?;
                return Ok(());
            };
            info!(
                "Removing from whitelist user (name: {:?}, id: {})",
                user.username(),
                user.id()
            );
            let removed = whitelist.lock().await.remove(user.id()).await?;
            if removed {
                message
                    .reply(InputMessage::text(
                        "Removed the user successfully.. We're not friends anymore 😭😭😭 ",
                    ))
                    .await?;
            } else {
                message
                    .reply(InputMessage::text("Who's dat? Idk them, do you? 👊🤨 "))
                    .await?;
            }
        }
        SuperuserCommand::WhitelistGet => {
            debug!("Showing whitelist");
            let whitelist = whitelist.lock().await;
            let user_ids = whitelist.users();
            let mut users_string = String::with_capacity(user_ids.len());
            for (i, user_id) in user_ids.iter().enumerate() {
                // apparently won't link properly if user did not interact with the bot:
                // https://stackoverflow.com/questions/40048452/telegram-bot-how-to-mention-user-by-its-id-not-its-username#comment108737106_46310679
                users_string.push_str(&format!(
                    "{}\\\n",
                    markdown::user_mention(
                        *user_id,
                        &format!("beeestieee {i} 😎 (the best one!!!)\n")
                    )
                ));
            }
            let reply_md = format!("List of my absolute besties 👯‍🌸️😎:\\\n{users_string}\n",);
            message.reply(InputMessage::markdown(&reply_md)).await?;
        }
    }
    Ok(())
}
