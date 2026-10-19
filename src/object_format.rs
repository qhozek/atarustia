//! HTML formatting with escaped text and attributes (grammers' unparser emits raw text).
use crate::{botapi::MessageEntity, output::escape};
pub fn html(text: &str, entities: &[MessageEntity]) -> String {
    let mut spans = entities
        .iter()
        .filter_map(|e| {
            let (open, close) = match e.kind.as_str() {
                "bold" => ("<b>".into(), "</b>"),
                "italic" => ("<i>".into(), "</i>"),
                "underline" => ("<u>".into(), "</u>"),
                "strikethrough" => ("<s>".into(), "</s>"),
                "spoiler" => ("<tg-spoiler>".into(), "</tg-spoiler>"),
                "code" => ("<code>".into(), "</code>"),
                "pre" => (
                    format!(
                        "<pre><code class=\"language-{}\">",
                        escape(e.language.as_deref().unwrap_or(""))
                    ),
                    "</code></pre>",
                ),
                "text_link" => (
                    format!("<a href=\"{}\">", escape(e.url.as_deref()?)),
                    "</a>",
                ),
                "text_mention" => (
                    format!(
                        "<a href=\"tg://user?id={}\">",
                        e.user_id.or_else(|| e.user.as_ref().map(|u| u.id))?
                    ),
                    "</a>",
                ),
                "custom_emoji" => (
                    format!(
                        "<tg-emoji emoji-id=\"{}\">",
                        escape(e.custom_emoji_id.as_deref()?)
                    ),
                    "</tg-emoji>",
                ),
                "blockquote" => ("<blockquote>".into(), "</blockquote>"),
                "expandable_blockquote" => ("<blockquote expandable>".into(), "</blockquote>"),
                "date_time" => (
                    format!(
                        "<tg-time unix=\"{}\" format=\"{}\">",
                        e.unix_time?,
                        escape(e.date_time_format.as_deref().unwrap_or(""))
                    ),
                    "</tg-time>",
                ),
                _ => return None,
            };
            Some((
                e.offset as usize,
                (e.offset + e.length) as usize,
                open,
                close,
            ))
        })
        .collect::<Vec<_>>();
    spans.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let total = text.encode_utf16().count();
    let mut out = String::new();
    let mut active = Vec::<usize>::new();
    let mut offset = 0;
    for ch in text.chars().chain(std::iter::once('\0')) {
        while let Some(&index) = active.last() {
            if spans[index].1 > offset {
                break;
            }
            out.push_str(spans[index].3);
            active.pop();
        }
        for (i, span) in spans.iter().enumerate().filter(|(_, s)| s.0 == offset) {
            out.push_str(&span.2);
            active.push(i);
        }
        if offset == total {
            break;
        }
        out.push_str(&escape(&ch.to_string()));
        offset += ch.len_utf16();
    }
    while let Some(i) = active.pop() {
        out.push_str(spans[i].3);
    }
    out
}
