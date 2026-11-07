//! Pinned Ataraxia Screen icons and custom-emoji HTML support missing in grammers.
use crate::{
    markup::{Appearance, Style},
    output::escape,
};
use grammers_client::{message::InputMessage, tl, update::Article};
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug)]
pub struct Icon {
    pub name: &'static str,
    pub id: i64,
    pub alt: &'static str,
    pub fallback: &'static str,
}
pub const ICONS: &[Icon] = &[
    Icon {
        name: "back",
        id: 5807619215321996754,
        alt: "⬅️",
        fallback: "‹",
    },
    Icon {
        name: "close",
        id: 5807692706507399432,
        alt: "✖️",
        fallback: "✕",
    },
    Icon {
        name: "search",
        id: 6021451978993834164,
        alt: "🔎",
        fallback: "🔍",
    },
    Icon {
        name: "settings",
        id: 6021582331251268218,
        alt: "⚙",
        fallback: "⚙",
    },
    Icon {
        name: "edit",
        id: 6024110353296660793,
        alt: "✏️",
        fallback: "✎",
    },
    Icon {
        name: "copy",
        id: 6021445064096486884,
        alt: "💾",
        fallback: "📋",
    },
    Icon {
        name: "reset",
        id: 5807900819147724161,
        alt: "🔄",
        fallback: "↺",
    },
    Icon {
        name: "check",
        id: 6019175208240289774,
        alt: "✅",
        fallback: "✓",
    },
    Icon {
        name: "on",
        id: 6041919344995209164,
        alt: "✅",
        fallback: "●",
    },
    Icon {
        name: "off",
        id: 5776428312414917091,
        alt: "⭕️",
        fallback: "○",
    },
    Icon {
        name: "add",
        id: 5807642902066634351,
        alt: "➕",
        fallback: "➕",
    },
    Icon {
        name: "remove",
        id: 6021413766669801212,
        alt: "🗑",
        fallback: "✕",
    },
    Icon {
        name: "eye",
        id: 6024008227564296298,
        alt: "👁",
        fallback: "👁",
    },
    Icon {
        name: "hide",
        id: 6021869252246510287,
        alt: "👁",
        fallback: "🙈",
    },
    Icon {
        name: "lock",
        id: 6019568309417023812,
        alt: "🔐",
        fallback: "🔒",
    },
    Icon {
        name: "secret",
        id: 6019290828759898301,
        alt: "🔑",
        fallback: "🔐",
    },
    Icon {
        name: "info",
        id: 6019094432790354513,
        alt: "💬",
        fallback: "ℹ️",
    },
    Icon {
        name: "warning",
        id: 6019102674832595118,
        alt: "⚠️",
        fallback: "⚠",
    },
    Icon {
        name: "module",
        id: 6024065724291488135,
        alt: "🧩",
        fallback: "🧩",
    },
    Icon {
        name: "core",
        id: 6023761060786346622,
        alt: "⚡️",
        fallback: "⛈",
    },
    Icon {
        name: "external",
        id: 6024106569430472546,
        alt: "📦",
        fallback: "📦",
    },
    Icon {
        name: "library",
        id: 6021836189588265800,
        alt: "📖",
        fallback: "📚",
    },
    Icon {
        name: "folder",
        id: 6021375494216226506,
        alt: "📁",
        fallback: "📁",
    },
    Icon {
        name: "list",
        id: 6026092845775985022,
        alt: "📋",
        fallback: "☰",
    },
    Icon {
        name: "number",
        id: 5807558162361884395,
        alt: "#️⃣",
        fallback: "#",
    },
    Icon {
        name: "text",
        id: 6026269596565117758,
        alt: "🔠",
        fallback: "🔤",
    },
    Icon {
        name: "choice",
        id: 6025946567779818123,
        alt: "🎛",
        fallback: "🎛",
    },
    Icon {
        name: "link",
        id: 6021452855167164269,
        alt: "⛓️",
        fallback: "🔗",
    },
    Icon {
        name: "user",
        id: 6024039683904772353,
        alt: "👤",
        fallback: "👤",
    },
    Icon {
        name: "bot",
        id: 6019462434178211398,
        alt: "📱",
        fallback: "🤖",
    },
    Icon {
        name: "guest",
        id: 6021690418398239007,
        alt: "👥",
        fallback: "🫂",
    },
    Icon {
        name: "command",
        id: 6021607757457660145,
        alt: "🖥",
        fallback: "⌨",
    },
    Icon {
        name: "placeholder",
        id: 6026071147601204905,
        alt: "⬜️",
        fallback: "▫️",
    },
    Icon {
        name: "page",
        id: 6021741567163767583,
        alt: "💾",
        fallback: "📄",
    },
    Icon {
        name: "help",
        id: 5807800879553715710,
        alt: "❓",
        fallback: "📖",
    },
];
pub fn icon(name: &str) -> Option<Icon> {
    ICONS.iter().find(|icon| icon.name == name).copied()
}
pub fn html(name: &str, premium: bool) -> String {
    match icon(name) {
        Some(icon) if premium => format!(
            "<tg-emoji emoji-id=\"{}\">{}</tg-emoji>",
            icon.id,
            escape(icon.alt)
        ),
        Some(icon) => escape(icon.fallback),
        None => escape(name),
    }
}
pub fn label(text: &str, name: &str, premium: bool) -> (String, Appearance) {
    match icon(name) {
        Some(icon) if premium => (
            if text.is_empty() {
                icon.fallback.into()
            } else {
                text.into()
            },
            Appearance {
                style: Style::Default,
                emoji_id: Some(icon.id),
            },
        ),
        Some(icon) => (
            format!("{} {text}", icon.fallback).trim().into(),
            Appearance::default(),
        ),
        None => (text.into(), Appearance::default()),
    }
}

static EMOJI: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
    r#"(?s)<(?:tg-emoji|emoji)\s+(?:emoji-id|document_id)\s*=\s*["']?(\d+)["']?\s*>(.*?)</(?:tg-emoji|emoji)>"#).unwrap()
});
pub fn normalize(source: &str, premium: bool) -> String {
    EMOJI
        .replace_all(source, |c: &regex::Captures<'_>| {
            if premium {
                format!("<tg-emoji emoji-id=\"{}\">{}</tg-emoji>", &c[1], &c[2])
            } else {
                c[2].into()
            }
        })
        .into_owned()
}
/// Parse text/formatting with grammers, and track custom emoji independently so
/// an emoji nested inside a user mention/link cannot corrupt the enclosing entity.
pub fn parse(source: &str) -> (String, Vec<tl::enums::MessageEntity>) {
    use html5ever::{
        tendril::StrTendril,
        tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer},
    };
    use std::cell::RefCell;
    #[derive(Default)]
    struct State {
        offset: i32,
        text: String,
        open: Vec<(i64, i32)>,
        entities: Vec<tl::enums::MessageEntity>,
    }
    struct Sink(RefCell<State>);
    impl TokenSink for Sink {
        type Handle = ();
        fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
            let mut state = self.0.borrow_mut();
            match token {
                Token::CharacterTokens(text) => {
                    state.offset += text.encode_utf16().count() as i32;
                    state.text.push_str(&text);
                }
                Token::TagToken(tag) if matches!(&*tag.name, "tg-emoji" | "emoji") => {
                    if tag.kind == TagKind::StartTag {
                        let id = tag
                            .attrs
                            .iter()
                            .find(|a| matches!(&*a.name.local, "emoji-id" | "document_id"))
                            .and_then(|a| a.value.parse::<i64>().ok())
                            .filter(|id| *id > 0)
                            .unwrap_or(0);
                        let offset = state.offset;
                        state.open.push((id, offset));
                    } else if let Some((id, offset)) = state.open.pop()
                        && id > 0
                        && state.offset > offset
                    {
                        let length = state.offset - offset;
                        state.entities.push(
                            tl::types::MessageEntityCustomEmoji {
                                offset,
                                length,
                                document_id: id,
                            }
                            .into(),
                        );
                    }
                }
                _ => {}
            }
            TokenSinkResult::Continue
        }
    }
    let tokenizer = Tokenizer::new(Sink(RefCell::new(State::default())), Default::default());
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(source));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let custom = tokenizer.sink.0.into_inner();
    let tl::enums::InputBotInlineMessage::Text(content) =
        Article::new("", InputMessage::new().html(source))
            .raw
            .send_message
    else {
        unreachable!()
    };
    let mut entities = content.entities.unwrap_or_default();
    // Do not apply guessed offsets if malformed HTML is parsed differently.
    if custom.text == content.message {
        entities.extend(custom.entities);
    }
    (content.message, entities)
}
pub fn input(source: &str) -> InputMessage {
    let (text, entities) = parse(source);
    InputMessage::new().text(text).fmt_entities(entities)
}
