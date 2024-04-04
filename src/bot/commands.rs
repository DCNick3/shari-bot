use crate::{
    bot::{markdown, whitelist::UserInfo, UserId},
    whatever::Whatever,
};
use grammers_client::{
    client::auth::InvocationError,
    types::{Chat, Message, User},
    Client, InputMessage,
};
use grammers_session::PackedChat;
use grammers_tl_types::types::MessageEntityBotCommand;
use indoc::indoc;
use snafu::{OptionExt, ResultExt, Snafu};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

#[derive(Debug)]
struct Alias(String);

impl Alias {
    /// If `None`, then the argument is not a user mention
    fn parse(arg: &str) -> Self {
        Alias(arg.trim_start_matches('@').to_string())
    }

    async fn resolve(&self, client: &Client) -> Result<Option<User>, Whatever> {
        let id = match client.resolve_username(&self.0).await {
            Ok(id) => id,
            Err(InvocationError::Rpc(r)) if r.code == 400 => None,
            Err(e) => return Err(e).whatever_context("Requesting username resolve"),
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
    Help,
}

#[derive(Debug, Snafu)]
enum CommandParseError {
    UnknownCommand,
    NoArgumentsProvided,
    #[allow(dead_code)]
    IncorrectArguments,
    ParsingError {
        inner: Whatever,
    },
}

impl From<Whatever> for CommandParseError {
    fn from(value: Whatever) -> Self {
        Self::ParsingError { inner: value }
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
        let command_text = String::from_utf16(command_text)
            .whatever_context::<_, Whatever>("Parsing command from message")?;
        let args = &text[(command.offset + command.length) as usize..];
        let args = String::from_utf16(args)
            .whatever_context::<_, Whatever>("Parsing arguments for the command from message")?;
        let mut args = args.split_whitespace();
        // expect??
        let command = command_text
            .split('@')
            .next()
            .whatever_context::<_, Whatever>("Split produced no elements for some reason")?;

        match command {
            "/whitelist" | "/whitelist_get" => Ok(Self::WhitelistGet),
            "/whitelist_add" => {
                let arg = args.next().ok_or(CommandParseError::NoArgumentsProvided)?;
                let alias = Alias::parse(arg);
                Ok(Self::WhitelistInsert(alias))
            }
            "/whitelist_remove" => {
                let arg = args.next().ok_or(CommandParseError::NoArgumentsProvided)?;
                let alias = Alias::parse(arg);
                Ok(Self::WhitelistRemove(alias))
            }
            "/help" => Ok(Self::Help),
            _ => Err(CommandParseError::UnknownCommand),
        }
    }
}

pub async fn handle_command(
    client: &Client,
    command: &MessageEntityBotCommand,
    message: &Message,
    whitelist: Arc<Mutex<crate::bot::whitelist::Whitelist>>,
) -> Result<(), Whatever> {
    let command = match SuperuserCommand::parse(command, message) {
        Ok(c) => c,
        Err(CommandParseError::UnknownCommand) => {
            message
                .reply(InputMessage::text(
                    "I don't know such command, /help might help",
                ))
                .await
                .whatever_context("Sending reply")?;
            return Ok(());
        }
        Err(CommandParseError::NoArgumentsProvided) => {
            message
                .reply(InputMessage::text("Missing argument(-s), /help might help"))
                .await
                .whatever_context("Sending reply")?;
            return Ok(());
        }
        Err(CommandParseError::IncorrectArguments) => {
            message
                .reply(InputMessage::text(
                    "I expected other arguments, /help might help",
                ))
                .await
                .whatever_context("Sending reply")?;
            return Ok(());
        }
        Err(CommandParseError::ParsingError { inner: e }) => return Err(e),
    };

    match command {
        SuperuserCommand::WhitelistInsert(alias) => {
            let Some(user) = alias.resolve(client).await? else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                message
                    .reply(InputMessage::text(
                        "All telegram doesn't know this person 🔍\nDid you type the name correctly?",
                    ))
                    .await
                    .whatever_context("Sending reply")?;
                return Ok(());
            };
            info!(
                "Adding into whitelist user (name: {:?}, id: {})",
                user.username(),
                user.id()
            );
            let Some(access_hash) = grammers_session::PackedChat::from(user.clone()).access_hash
            else {
                warn!("no access hash found for user id {:?}", user.id());
                message
                    .reply(InputMessage::text(format!(
                        "Cannot access info of {:?}",
                        user.username()
                    )))
                    .await
                    .whatever_context("Sending reply")?;
                return Ok(());
            };
            let added = whitelist
                .lock()
                .await
                .insert(UserId(user.id()), UserInfo { access_hash })
                .await
                .whatever_context("Inserting user into whitelist")?;
            if added {
                message
                    .reply(InputMessage::text(
                        "Added the user successfully! ✨ Now they can use this bot ✨",
                    ))
                    .await
                    .whatever_context("Sending reply")?;
            } else {
                message
                    .reply(InputMessage::text(
                        "✨ I already know this person! (or bot 🤔) ✨",
                    ))
                    .await
                    .whatever_context("Sending reply")?;
            }
        }
        SuperuserCommand::WhitelistRemove(alias) => {
            let Some(user) = alias.resolve(client).await? else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                message
                    .reply(InputMessage::text(
                        "All telegram doesn't know this person 🔍\nDid you type the name correctly?",
                    ))
                    .await
                    .whatever_context("Sending reply")?;
                return Ok(());
            };
            info!(
                "Removing from whitelist user (name: {:?}, id: {})",
                user.username(),
                user.id()
            );
            let removed = whitelist
                .lock()
                .await
                .remove(UserId(user.id()))
                .await
                .whatever_context("Removing user from whitelist")?;
            if removed.is_some() {
                message
                    .reply(InputMessage::text(
                        "Removed the user successfully.. We're not friends anymore 😭😭😭 ",
                    ))
                    .await
                    .whatever_context("Sending reply")?;
            } else {
                message
                    .reply(InputMessage::text("Who's dat? Idk them, do you? 👊🤨 "))
                    .await
                    .whatever_context("Sending reply")?;
            }
        }
        SuperuserCommand::WhitelistGet => {
            debug!("Showing whitelist");
            let whitelist = whitelist.lock().await;
            let user_ids = whitelist.users();
            let mut users_string = String::with_capacity(user_ids.len());
            for (user_id, user_info) in user_ids.iter() {
                let chat = client
                    .unpack_chat(PackedChat {
                        ty: grammers_session::PackedType::User,
                        id: user_id.0,
                        access_hash: Some(user_info.access_hash),
                    })
                    .await
                    .whatever_context("Unpacking chat")?;

                let user_tag = match chat.username() {
                    Some(name) => format!("@{}", name),
                    // apparently won't link properly if user did not interact with the bot:
                    // https://stackoverflow.com/questions/40048452/telegram-bot-how-to-mention-user-by-its-id-not-its-username#comment108737106_46310679
                    None => markdown::user_mention(*user_id, "<cringe cuteness drowning femboy>"),
                };

                users_string.push_str(&format!(
                    "beeestieee {} 😎 (the best one!!!)\n\\\n",
                    user_tag
                ));
            }
            let reply_md = format!("List of my absolute besties 👯‍🌸️😎:\\\n{users_string}\n",);
            message
                .reply(InputMessage::markdown(&reply_md))
                .await
                .whatever_context("Sending reply")?;
        }
        SuperuserCommand::Help => {
            message
                .reply(InputMessage::text(indoc!(
                    r#"
                /whitelist - show users in whitelist
                /whitelist_add @username - add user to the whitelist 
                /whitelist_remove @username - remove user from the whitelist 
                /help - show this message
                "#
                )))
                .await
                .whatever_context("Sending reply")?;
        }
    }
    Ok(())
}
