//! Contains all the text messages to be sent to the user

use displaydoc::Display;

#[derive(Debug, Display)]
pub enum Lang {
    /// sowwy i am not awwowed to spek with pepel i donbt now (yet) (/ω＼)
    NoAccess,
    /// Sen me smth with a URL in it and I wiww try to figuwe it out UwU
    NoUrl,
    /// I donbt no ho to doload tis url((999
    UnsupportedUrl,

    /// Wowking~   (ﾉ>ω<)ﾉ
    StatusWorking,
    /// Gettinb vid linkie (；⌣̀_⌣́)～
    StatusGettingLink,

    /// did it!1!1!  (ﾉ>ω<)ﾉ :｡･:*:･ﾟ’★,｡･:*:･ﾟ’☆
    ResultSuccess,
    /// \[Took too long to download & upload the video, maybe the file is too large or the bot is under heavy load\]
    ResultErrorTimeout,
    /**
    ewwow(((99  .･ﾟﾟ･(／ω＼)･ﾟﾟ･.

    {0}*/
    ResultGenericError(String),

    /// I donbt understan tis command ☆⌒(> _ <) \[/help might help\]
    CommandUnknown,
    /// Tis command needs args ☆⌒(> _ <) \[/help might help\]
    CommandNeedsArgs,
    /// Tis command needs different args ☆⌒(> _ <) \[/help might help\]
    CommandIncorrectArgs,
    /**
    /whitelist - show users in whitelist
    /whitelist_add @username - add user to the whitelist
    /whitelist_remove @username - remove user from the whitelist
    /help - show this message*/
    CommandHelp,

    /// All telegram doesn't know this person 🔍\nDid you type the name correctly?
    WhitelistErrorAliasResolve,
    /// Cannot access info of {0}
    WhitelistErrorNoAccessHash(i64),
    /// Added the user successfully! ✨ Now they can use this bot ✨
    WhitelistAddOk,
    /// ✨ I already know this person! (or bot 🤔) ✨
    WhitelistAddKnown,
    /// Removed the user successfully.. We're not friends anymore 😭😭😭
    WhitelistRemoveOk,
    /// Who's dat? Idk them, do you? 👊🤨
    WhitelistRemoveUnknown,

    /// List of my absolute besties 👯‍🌸️😎
    WhitelistListHead,
    /// beeestieee {0} 😎 (the best one!!!)
    WhitelistListItem(String),
}

impl From<Lang> for grammers_client::InputMessage {
    fn from(value: Lang) -> Self {
        let str = format!("{value}");
        grammers_client::InputMessage::text(str)
    }
}
