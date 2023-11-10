use crate::bot::UserId;
use anyhow::Context;
use grammers_client::types::Message;
use grammers_client::InputMessage;
use grammers_tl_types::enums::MessageEntity;
use grammers_tl_types::types::MessageEntityBotCommand;
use tracing::{debug, info, warn};

#[derive(Debug)]
struct Alias {
    text: String,
    user_id: UserId,
}

impl Alias {
    /// If `None`, then the argument is not a user mention
    fn parse(message: &Message, arg: &str) -> Option<Self> {
        let text = message.text();
        let text = text.encode_utf16().collect::<Vec<_>>();
        let mut mentions = message
            .fmt_entities()
            .into_iter()
            .flatten()
            .filter_map(|e| match e {
                MessageEntity::MentionName(m) => Some(m),
                _ => None,
            });
        mentions.find_map(|mention| {
            let name = &text[mention.offset as usize..(mention.offset + mention.length) as usize];
            let Ok(name) = String::from_utf16(name) else {
                warn!("Could not parse mention text form {:?}", name);
                return None;
            };
            if name == arg {
                Some(Alias {
                    text: name,
                    user_id: mention.user_id,
                })
            } else {
                None
            }
        })
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
    command: &MessageEntityBotCommand,
    message: &Message,
) -> anyhow::Result<()> {
    let command = match SuperuserCommand::parse(command, message) {
        Ok(c) => c,
        Err(CommandParseError::UnknownCommand) => {
            message
                .reply(InputMessage::text(
                    "I don't know such command, /help might help",
                ))
                .await?;
        }
        Err(CommandParseError::NoArgumentsProvided) => {
            message
                .reply(InputMessage::text("Missing argument(-s), /help might help"))
                .await?;
        }
        Err(CommandParseError::IncorrectArguments) => {
            message
                .reply(InputMessage::text(
                    "I expected other arguments, /help might help",
                ))
                .await?;
        }
        Err(CommandParseError::ParsingError(e)) => return Err(e.context("Parsing command")),
    };

    match command {
        SuperuserCommand::WhitelistInsert(user) => {
            info!("Adding into whitelist user {:?}", user);
        }
        SuperuserCommand::WhitelistRemove(user) => {
            info!("Removing from whitelist user {:?}", user);
        }
        SuperuserCommand::WhitelistGet => {
            debug!("Showing whitelist");
        }
    }
    Ok(())
}
