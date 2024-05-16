use std::sync::Arc;

use grammers_client::{
    client::auth::InvocationError,
    types::{Chat, Message, User},
    Client, InputMessage,
};
use grammers_session::PackedChat;
use grammers_tl_types::types::MessageEntityBotCommand;
use snafu::{OptionExt, ResultExt, Snafu};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::{
    bot::{lang::Lang, markdown, whitelist::UserInfo, UserId},
    whatever::Whatever,
};

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
    #[snafu(whatever, display("{message}"))]
    InternalError {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error + Send + Sync>, Some)))]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
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
            String::from_utf16(command_text).whatever_context("Parsing command from message")?;
        let args = &text[(command.offset + command.length) as usize..];
        let args = String::from_utf16(args)
            .whatever_context("Parsing arguments for the command from message")?;
        let mut args = args.split_whitespace();
        // expect??
        let command = command_text
            .split('@')
            .next()
            .whatever_context("Split produced no elements for some reason")?;

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
) -> Result<InputMessage, Whatever> {
    let command = match SuperuserCommand::parse(command, message) {
        Ok(command) => command,
        Err(e) => {
            return Ok(match e {
                CommandParseError::UnknownCommand => Lang::CommandUnknown,
                CommandParseError::NoArgumentsProvided => Lang::CommandNeedsArgs,
                CommandParseError::IncorrectArguments => Lang::CommandIncorrectArgs,
                e => return Err::<InputMessage, _>(e).whatever_context("Parsing command"),
            }
            .into())
        }
    };

    let reply = match command {
        SuperuserCommand::WhitelistInsert(alias) => {
            let Some(user) = alias
                .resolve(client)
                .await
                .whatever_context("Resolving the user alias")?
            else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                return Ok(Lang::WhitelistErrorAliasResolve.into());
            };
            info!(
                "Adding into whitelist user (name: {:?}, id: {})",
                user.username(),
                user.id()
            );
            let Some(access_hash) = grammers_session::PackedChat::from(user.clone()).access_hash
            else {
                warn!("no access hash found for user id {:?}", user.id());
                return Ok(Lang::WhitelistErrorNoAccessHash(user.id()).into());
            };
            let added = whitelist
                .lock()
                .await
                .insert(UserId(user.id()), UserInfo { access_hash })
                .await
                .whatever_context("Inserting user into whitelist")?;
            if added {
                Lang::WhitelistAddOk
            } else {
                Lang::WhitelistAddKnown
            }
        }
        SuperuserCommand::WhitelistRemove(alias) => {
            let Some(user) = alias
                .resolve(client)
                .await
                .whatever_context("Resolving the user alias")?
            else {
                info!("Got username, but couldn't resolve: {}", alias.0);
                return Ok(Lang::WhitelistErrorAliasResolve.into());
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
                Lang::WhitelistRemoveOk
            } else {
                Lang::WhitelistRemoveUnknown
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

                users_string.push_str(&format!("{}\n\\\n", Lang::WhitelistListItem(user_tag)));
            }
            let reply_md = format!("{}:\\\n{}\n", Lang::WhitelistListHead, users_string);

            return Ok(InputMessage::markdown(&reply_md));
        }
        SuperuserCommand::Help => Lang::CommandHelp,
    };
    Ok(reply.into())
}
