#![allow(unused)]

//! Utils for working with the [Markdown V2 message style][spec].
//!
//! [spec]: https://core.telegram.org/bots/api#markdownv2-style

/// Applies the bold font style to the string.
///
/// Passed string will not be automatically escaped because it can contain
/// nested markup.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn bold(s: &str) -> String {
    format!("*{s}*")
}

/// Applies the italic font style to the string.
///
/// Can be safely used with `utils::markdown::underline()`.
/// Passed string will not be automatically escaped because it can contain
/// nested markup.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn italic(s: &str) -> String {
    if s.starts_with("__") && s.ends_with("__") {
        format!(r"_{}\r__", &s[..s.len() - 1])
    } else {
        format!("_{s}_")
    }
}

/// Applies the underline font style to the string.
///
/// Can be safely used with `utils::markdown::italic()`.
/// Passed string will not be automatically escaped because it can contain
/// nested markup.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn underline(s: &str) -> String {
    // In case of ambiguity between italic and underline entities
    // ‘__’ is always greedily treated from left to right as beginning or end of
    // underline entity, so instead of ___italic underline___ we should use
    // ___italic underline_\r__, where \r is a character with code 13, which
    // will be ignored.
    if s.starts_with('_') && s.ends_with('_') {
        format!(r"__{s}\r__")
    } else {
        format!("__{s}__")
    }
}

/// Applies the strikethrough font style to the string.
///
/// Passed string will not be automatically escaped because it can contain
/// nested markup.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn strike(s: &str) -> String {
    format!("~{s}~")
}

/// Builds an inline link with an anchor.
///
/// Escapes `)` and ``` characters inside the link url.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn link(url: &str, text: &str) -> String {
    format!("[{}]({})", text, escape_link_url(url))
}

/// Builds an inline user mention link with an anchor.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn user_mention(user_id: i64, text: &str) -> String {
    link(format!("tg://user?id={user_id}").as_str(), text)
}

/// Formats the code block.
///
/// Escapes ``` and `\` characters inside the block.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn code_block(code: &str) -> String {
    format!("```\n{}\n```", escape_code(code))
}

/// Formats the code block with a specific language syntax.
///
/// Escapes ``` and `\` characters inside the block.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn code_block_with_lang(code: &str, lang: &str) -> String {
    format!("```{}\n{}\n```", escape(lang), escape_code(code))
}

/// Formats the string as an inline code.
///
/// Escapes ``` and `\` characters inside the block.
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn code_inline(s: &str) -> String {
    format!("`{}`", escape_code(s))
}

/// Escapes the string to be shown "as is" within the Telegram [Markdown
/// v2][spec] message style.
///
/// [spec]: https://core.telegram.org/bots/api#html-style
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn escape(s: &str) -> String {
    s.replace('_', r"\_")
        .replace('*', r"\*")
        .replace('[', r"\[")
        .replace(']', r"\]")
        .replace('(', r"\(")
        .replace(')', r"\)")
        .replace('~', r"\~")
        .replace('`', r"\`")
        .replace('>', r"\>")
        .replace('#', r"\#")
        .replace('+', r"\+")
        .replace('-', r"\-")
        .replace('=', r"\=")
        .replace('|', r"\|")
        .replace('{', r"\{")
        .replace('}', r"\}")
        .replace('.', r"\.")
        .replace('!', r"\!")
}

/// Escapes all markdown special characters specific for the inline link URL
/// (``` and `)`).
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn escape_link_url(s: &str) -> String {
    s.replace('`', r"\`").replace(')', r"\)")
}

/// Escapes all markdown special characters specific for the code block (``` and
/// `\`).
#[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
              without using its output does nothing useful"]
pub fn escape_code(s: &str) -> String {
    s.replace('\\', r"\\").replace('`', r"\`")
}

// #[must_use = "This function returns a new string, rather than mutating the argument, so calling it \
//               without using its output does nothing useful"]
// pub fn user_mention_or_link(user: &User) -> String {
//     match user.mention() {
//         Some(mention) => mention,
//         None => link(user.url().as_str(), &escape(&user.full_name())),
//     }
// }
