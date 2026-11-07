//! Rich screen source normalization and structured Telegram-HTML fallback.
//! Mirrors upstream inline/screens.py rather than deleting rich tags with regex.
use crate::{
    forms::{Action, Key},
    markup::Appearance,
    output::escape,
};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer},
};
use std::{cell::RefCell, collections::BTreeMap, sync::LazyLock};

pub const ROW_LIMIT: usize = 8;
pub const KEYBOARD_LIMIT: usize = 100;
static BLOCKS: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
    r"\n*(</?(?i-u:details|summary|table|tr|th|td|blockquote|tg-button-row|img|h[1-6]|hr|pre|ul|ol|li)\b[^>]*>)\n*").unwrap()
});
static PRE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?i-u:<pre)\b[^>]*>(?s:.*?)(?i-u:</pre>)").unwrap());
static TAGS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"<[^>]+>").unwrap());

pub fn to_rich(source: &str) -> String {
    let source = BLOCKS.replace_all(source.trim_matches('\n'), "$1");
    let mut out = String::new();
    let mut offset = 0;
    for found in PRE.find_iter(&source) {
        out.push_str(&source[offset..found.start()].replace('\n', "<br>"));
        out.push_str(found.as_str());
        offset = found.end();
    }
    out.push_str(&source[offset..].replace('\n', "<br>"));
    out
}

#[derive(Clone)]
pub struct FallbackKey {
    pub key: Key,
    pub appearance: Appearance,
}
pub struct Fallback {
    pub text: String,
    pub rows: Vec<Vec<FallbackKey>>,
}
impl Fallback {
    pub fn keys(&self) -> Vec<Vec<Key>> {
        self.rows
            .iter()
            .map(|r| r.iter().map(|k| k.key.clone()).collect())
            .collect()
    }
}
struct Button {
    attrs: BTreeMap<String, String>,
    label: String,
}
#[derive(Default)]
struct Plain {
    out: Vec<String>,
    rows: Vec<Vec<FallbackKey>>,
    loose: Vec<FallbackKey>,
    button: Option<Button>,
    row: Option<Vec<FallbackKey>>,
    details: Vec<bool>,
    quotes: Vec<bool>,
    cells: usize,
    header: bool,
    summary: Option<usize>,
}
fn kept(tag: &str) -> bool {
    matches!(
        tag,
        "b" | "strong"
            | "i"
            | "em"
            | "u"
            | "s"
            | "del"
            | "code"
            | "pre"
            | "a"
            | "tg-spoiler"
            | "tg-emoji"
    )
}
fn heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}
fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
impl Plain {
    fn emit(&mut self, text: impl AsRef<str>) {
        if let Some(button) = &mut self.button {
            button.label.push_str(text.as_ref());
        } else {
            self.out.push(text.as_ref().into());
        }
    }
    fn newline(&mut self) {
        if self.button.is_none() && self.out.last().is_some_and(|s| !s.ends_with('\n')) {
            self.out.push("\n".into());
        }
    }
    fn flush(&mut self) {
        self.rows.extend(self.loose.chunks(2).map(|r| r.to_vec()));
        self.loose.clear();
    }
    fn unquote(&mut self) {
        while let Some(last) = self.out.last_mut() {
            if !last.ends_with('\n') {
                break;
            }
            last.pop();
            if last.is_empty() {
                self.out.pop();
            }
        }
        self.emit("</blockquote>");
    }
    fn start(&mut self, tag: &str, attrs: BTreeMap<String, String>) {
        match tag {
            "tg-button" => {
                self.button = Some(Button {
                    attrs,
                    label: String::new(),
                })
            }
            "tg-button-row" => {
                self.flush();
                self.row = Some(vec![]);
            }
            t if kept(t) => {
                let mut html = format!("<{tag}");
                for (name, value) in attrs {
                    html += &format!(" {name}=\"{}\"", escape(&value).replace('\'', "&#x27;"));
                }
                html.push('>');
                self.emit(html);
            }
            "br" => self.emit("\n"),
            "details" => {
                self.flush();
                self.newline();
                self.details.push(attrs.contains_key("open"));
                self.quotes.push(false);
            }
            "summary" => self.summary = Some(self.out.len()),
            "blockquote" => {
                self.flush();
                self.newline();
                let shown = !self.quotes.iter().any(|q| *q);
                self.quotes.push(shown);
                if shown {
                    self.emit(if attrs.contains_key("expandable") {
                        "<blockquote expandable>"
                    } else {
                        "<blockquote>"
                    });
                }
            }
            "table" | "tr" => {
                if tag == "table" {
                    self.flush();
                }
                self.newline();
                self.cells = 0;
            }
            "th" | "td" => {
                if self.cells > 0 {
                    self.emit(if self.header { ": " } else { " · " });
                }
                self.header = tag == "th" && self.cells == 0;
                self.cells += 1;
                if tag == "th" {
                    self.emit("<b>");
                }
            }
            t if heading(t) => {
                self.newline();
                self.emit("<b>");
            }
            "hr" | "p" | "div" => self.newline(),
            "li" => {
                self.newline();
                self.emit("• ");
            }
            _ => {}
        }
    }
    fn end(&mut self, tag: &str, taps: &BTreeMap<String, FallbackKey>) {
        match tag {
            "tg-button" => self.finish_button(taps),
            "tg-button-row" => {
                if let Some(row) = self.row.take() {
                    self.rows.extend(row.chunks(ROW_LIMIT).map(|r| r.to_vec()));
                }
                self.newline();
            }
            t if kept(t) => self.emit(format!("</{tag}>")),
            "summary" => {
                if let Some(summary) = self.summary.take()
                    && !self.out[summary..].join("").contains("<b>")
                {
                    self.out.insert(summary, "<b>".into());
                    self.emit("</b>");
                }
                self.emit("\n");
                if !self.details.is_empty() && !self.quotes.iter().any(|q| *q) {
                    if let Some(q) = self.quotes.last_mut() {
                        *q = true;
                    }
                    self.emit(if *self.details.last().unwrap() {
                        "<blockquote>"
                    } else {
                        "<blockquote expandable>"
                    });
                }
            }
            "details" => {
                self.details.pop();
                if self.quotes.pop() == Some(true) {
                    self.unquote();
                }
                self.newline();
            }
            "blockquote" => {
                if self.quotes.pop() == Some(true) {
                    self.unquote();
                }
                self.newline();
            }
            "th" => self.emit("</b>"),
            "table" | "tr" | "p" | "div" | "li" => self.newline(),
            t if heading(t) => {
                self.emit("</b>");
                self.newline();
            }
            _ => {}
        }
    }
    fn finish_button(&mut self, taps: &BTreeMap<String, FallbackKey>) {
        let Some(button) = self.button.take() else {
            return;
        };
        let text = decode_entities(&TAGS.replace_all(&button.label, ""))
            .trim()
            .to_owned();
        let text = if text.is_empty() { "·".into() } else { text };
        let kind = button.attrs.get("type").map(String::as_str).unwrap_or("");
        let known = button
            .attrs
            .get("data")
            .and_then(|data| taps.get(data))
            .cloned();
        let key = known.unwrap_or_else(|| match kind {
            "callback_data" => FallbackKey {
                key: Key {
                    text: text.clone(),
                    action: Some(Action::Disabled),
                    url: None,
                },
                appearance: Default::default(),
            },
            "copy_text" => {
                let value = button
                    .attrs
                    .get("text")
                    .filter(|v| !v.is_empty())
                    .cloned()
                    .unwrap_or_else(|| " ".into());
                let label = if text.chars().count() <= 2 {
                    format!("{text} {value}").chars().take(64).collect()
                } else {
                    text.clone()
                };
                FallbackKey {
                    key: Key {
                        text: label,
                        action: Some(Action::Copy { text: value }),
                        url: None,
                    },
                    appearance: Default::default(),
                }
            }
            "url" => FallbackKey {
                key: Key {
                    text: text.clone(),
                    url: Some(button.attrs.get("url").cloned().unwrap_or_default()),
                    action: None,
                },
                appearance: Default::default(),
            },
            "switch_inline_query" | "switch_inline_query_current_chat" => FallbackKey {
                key: Key {
                    text: text.clone(),
                    url: None,
                    action: Some(Action::SwitchInline {
                        query: button.attrs.get("query").cloned().unwrap_or_default(),
                        same_chat: kind == "switch_inline_query_current_chat",
                    }),
                },
                appearance: Default::default(),
            },
            _ => FallbackKey {
                key: Key {
                    text: text.clone(),
                    action: Some(Action::Disabled),
                    url: None,
                },
                appearance: Default::default(),
            },
        });
        if let Some(row) = &mut self.row {
            row.push(key);
        } else if kind == "callback_data" {
            self.emit(button.label);
            self.loose.push(key);
        } else if matches!(
            kind,
            "copy_text" | "url" | "switch_inline_query" | "switch_inline_query_current_chat"
        ) {
            self.loose.push(key);
        } else {
            self.emit(format!("⟨{}⟩", button.label));
        }
    }
}
struct Sink<'a> {
    plain: RefCell<Plain>,
    taps: &'a BTreeMap<String, FallbackKey>,
}
impl TokenSink for Sink<'_> {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut plain = self.plain.borrow_mut();
        match token {
            Token::CharacterTokens(text) => plain.emit(escape_text(&text)),
            Token::NullCharacterToken => plain.emit("�"),
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                if tag.kind == TagKind::EndTag {
                    plain.end(&name, self.taps);
                } else {
                    let attrs = tag
                        .attrs
                        .into_iter()
                        .map(|a| (a.name.local.to_string(), a.value.to_string()))
                        .collect();
                    if tag.self_closing && !matches!(name.as_str(), "br" | "hr") { /* upstream ignores non-br/hr startend tags */
                    } else {
                        plain.start(&name, attrs);
                    }
                }
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}
pub fn to_plain(source: &str, taps: &BTreeMap<String, FallbackKey>) -> Fallback {
    let sink = Sink {
        plain: RefCell::new(Plain::default()),
        taps,
    };
    let tokenizer = Tokenizer::new(sink, Default::default());
    let buffer = BufferQueue::default();
    buffer.push_back(StrTendril::from(BLOCKS.replace_all(source, "$1").as_ref()));
    let _ = tokenizer.feed(&buffer);
    tokenizer.end();
    let mut plain = tokenizer.sink.plain.into_inner();
    plain.flush();
    static TRAILING: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[ \t]+\n").unwrap());
    static BLANKS: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"\n{3,}").unwrap());
    let text = TRAILING.replace_all(&plain.out.join(""), "\n").into_owned();
    Fallback {
        text: BLANKS.replace_all(&text, "\n\n").trim().into(),
        rows: plain.rows,
    }
}

/// Decode labels/attributes with the same HTML tokenizer, without reparsing values as markup.
fn decode_entities(source: &str) -> String {
    struct Text(RefCell<String>);
    impl TokenSink for Text {
        type Handle = ();
        fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
            if let Token::CharacterTokens(text) = token {
                self.0.borrow_mut().push_str(&text);
            }
            TokenSinkResult::Continue
        }
    }
    let tokenizer = Tokenizer::new(Text(RefCell::new(String::new())), Default::default());
    let input = BufferQueue::default();
    input.push_back(source.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    tokenizer.sink.0.into_inner()
}

/// Resolve only locally declared actions. Raw callback data cannot fabricate core actions.
pub fn fallback(source: &str, taps: &[Key], appearances: &[Appearance]) -> Fallback {
    let mut bound = source.to_owned();
    let mut map = BTreeMap::new();
    let nonce = crate::web::random_token();
    for (index, key) in taps.iter().enumerate() {
        let token = format!("rich-fallback:{nonce}:{index}");
        let style = appearances.get(index).cloned().unwrap_or_default();
        map.insert(
            token.clone(),
            FallbackKey {
                key: key.clone(),
                appearance: style.clone(),
            },
        );
        let callback = !matches!(
            key.action,
            Some(Action::Copy { .. } | Action::Disabled | Action::SwitchInline { .. })
        ) && key.action.is_some();
        let html = crate::rich::styled_button_html(
            &key.text,
            callback.then_some(token.as_str()),
            key.url.as_deref(),
            key.action.as_ref(),
            Some(&style),
        );
        // Identity is independent of label and button kind (copy/URL/input/disabled too).
        let html = if callback {
            html
        } else {
            html.replacen('>', &format!(" data=\"{token}\">"), 1)
        };
        bound = bound.replace(&crate::rich::marker(index), &html);
    }
    to_plain(&bound, &map)
}

/// Split parsed text, not HTML source: entities are clipped at UTF-16 page boundaries.
pub fn plain_pages(html: &str) -> anyhow::Result<Vec<grammers_client::message::InputMessage>> {
    use crate::botapi::MessageEntity;
    use grammers_client::{message::InputMessage, tl, update::Article};
    let article = Article::new("", crate::emoji::input(html));
    let tl::enums::InputBotInlineMessage::Text(content) = article.raw.send_message else {
        unreachable!()
    };
    let entities: Vec<_> = content
        .entities
        .unwrap_or_default()
        .iter()
        .filter_map(MessageEntity::from_tl)
        .collect();
    let mut start = 0i32;
    crate::output::pages(&content.message)
        .into_iter()
        .map(|text| {
            let end = start + text.encode_utf16().count() as i32;
            let clipped = entities
                .iter()
                .filter_map(|entity| {
                    let left = entity.offset.max(start);
                    let right = (entity.offset + entity.length).min(end);
                    if left >= right {
                        return None;
                    }
                    let mut entity = entity.clone();
                    entity.offset = left - start;
                    entity.length = right - left;
                    Some(entity.to_tl(&text))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            start = end;
            Ok(InputMessage::new()
                .text(if text.is_empty() {
                    "\u{2060}".into()
                } else {
                    text
                })
                .fmt_entities(clipped))
        })
        .collect()
}

/// Only a server-side rich/format rejection warrants retrying as plain text.
/// Network, FloodWait, authorization and unrelated RPC errors must propagate.
pub fn rejected(error: &anyhow::Error) -> bool {
    if let Some(grammers_client::InvocationError::Rpc(rpc)) =
        error.downcast_ref::<grammers_client::InvocationError>()
    {
        return matches!(
            rpc.name.as_str(),
            "RICH_MESSAGE_INVALID"
                | "RICH_MESSAGE_NOT_ALLOWED"
                | "CUSTOM_EMOJI_INVALID"
                | "PREMIUM_ACCOUNT_REQUIRED"
                | "ENTITY_BOUNDS_INVALID"
                | "ENTITIES_TOO_LONG"
        );
    }
    if let Some(api) = error.downcast_ref::<crate::botapi::ApiError>() {
        return rejected_api(api);
    }
    false
}
pub fn rejected_api(api: &crate::botapi::ApiError) -> bool {
    matches!(api.kind, crate::botapi::ErrorKind::BadRequest)
        && [
            "rich message",
            "rich_message",
            "custom emoji",
            "custom_emoji_invalid",
            "premium_account_required",
            "can't parse entities",
            "entity_bounds_invalid",
            "entities_too_long",
        ]
        .iter()
        .any(|name| api.description.to_ascii_lowercase().contains(name))
}
