//! Rich-screen builder with bound in-text callback buttons and a plain keyboard fallback.
use crate::{
    forms::{Key, Screen},
    output::escape,
};
use anyhow::{Result, ensure};
#[derive(Default)]
pub struct RichScreen {
    parts: Vec<String>,
    buttons: Vec<Key>,
    plain: Vec<String>,
    appearances: Vec<crate::markup::Appearance>,
}
impl RichScreen {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn heading(&mut self, text: &str) -> &mut Self {
        self.parts.push(format!("<h2>{}</h2>", escape(text)));
        self.plain.push(text.into());
        self
    }
    pub fn text(&mut self, text: &str) -> &mut Self {
        self.parts.push(escape(text));
        self.plain.push(text.into());
        self
    }
    pub fn link(&mut self, label: &str, url: &str) -> Result<&mut Self> {
        let parsed = reqwest::Url::parse(url)?;
        ensure!(
            matches!(parsed.scheme(), "https" | "tg"),
            "rich link must use HTTPS or tg"
        );
        self.parts
            .push(format!("<a href=\"{}\">{}</a>", escape(url), escape(label)));
        self.plain.push(format!("{label} ({url})"));
        Ok(self)
    }
    pub fn list(&mut self, items: &[String], ordered: bool) -> &mut Self {
        let tag = if ordered { "ol" } else { "ul" };
        let body = items
            .iter()
            .map(|i| format!("<li>{}</li>", escape(i)))
            .collect::<String>();
        self.parts.push(format!("<{tag}>{body}</{tag}>"));
        self.plain.extend(items.iter().enumerate().map(|(i, s)| {
            if ordered {
                format!("{}. {s}", i + 1)
            } else {
                format!("• {s}")
            }
        }));
        self
    }
    pub fn code(&mut self, source: &str, language: &str) -> Result<&mut Self> {
        ensure!(
            language.len() <= 32
                && language
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
            "invalid code language"
        );
        self.parts.push(format!(
            "<pre><code class=\"language-{}\">{}</code></pre>",
            escape(language),
            escape(source)
        ));
        self.plain.push(source.into());
        Ok(self)
    }
    /// Trusted rendering code only: escape dynamic values before passing raw HTML.
    pub fn html(&mut self, html: &str, plain: &str) -> &mut Self {
        self.parts.push(html.into());
        self.plain.push(plain.into());
        self
    }
    pub fn tap(&mut self, key: Key) -> Result<String> {
        self.styled_tap(key, crate::markup::Appearance::default())
    }
    pub fn styled_tap(
        &mut self,
        key: Key,
        appearance: crate::markup::Appearance,
    ) -> Result<String> {
        appearance.validate()?;
        ensure!(self.buttons.len() < 90, "too many rich buttons");
        let index = self.buttons.len();
        self.buttons.push(key);
        self.appearances.push(appearance);
        Ok(marker(index))
    }
    pub fn emoji(&mut self, id: i64, alt: &str) -> Result<&mut Self> {
        ensure!(
            id > 0 && !alt.is_empty() && alt.chars().count() <= 16,
            "invalid custom emoji or fallback text"
        );
        self.parts.push(format!(
            "<tg-emoji emoji-id=\"{id}\">{}</tg-emoji>",
            escape(alt)
        ));
        self.plain.push(alt.into());
        Ok(self)
    }
    pub fn table(&mut self, rows: &[Vec<String>]) -> &mut Self {
        let body = rows
            .iter()
            .map(|row| {
                format!(
                    "<tr>{}</tr>",
                    row.iter()
                        .map(|c| format!("<td>{}</td>", escape(c)))
                        .collect::<String>()
                )
            })
            .collect::<String>();
        self.parts
            .push(format!("<table bordered compact>{body}</table>"));
        self.plain.extend(rows.iter().map(|r| r.join(" · ")));
        self
    }
    pub fn details(&mut self, title: &str, body: &str) -> &mut Self {
        self.parts.push(format!(
            "<details><summary>{}</summary>{}</details>",
            escape(title),
            escape(body)
        ));
        self.plain.push(format!("{title}\n{body}"));
        self
    }
    pub fn finish(self) -> Screen {
        Screen {
            text: self.plain.join("\n"),
            rich_html: Some(self.parts.join("<br>")),
            rich_buttons: self.buttons,
            rich_button_appearance: self.appearances,
            ..Default::default()
        }
    }
}
pub fn marker(index: usize) -> String {
    format!("{{{{atarustia:tap:{index}}}}}")
}
pub fn button_html(
    label: &str,
    token: Option<&str>,
    url: Option<&str>,
    action: Option<&crate::forms::Action>,
) -> String {
    let text = escape(label);
    if let Some(token) = token {
        return format!(
            "<tg-button type=\"callback_data\" data=\"{}\">{text}</tg-button>",
            escape(token)
        );
    }
    if let Some(url) = url {
        return format!(
            "<tg-button type=\"url\" url=\"{}\">{text}</tg-button>",
            escape(url)
        );
    }
    match action {
        Some(crate::forms::Action::Copy { text: copy }) => format!(
            "<tg-button type=\"copy_text\" text=\"{}\">{text}</tg-button>",
            escape(copy)
        ),
        _ => format!("<tg-button type=\"disabled\">{text}</tg-button>"),
    }
}
pub fn styled_button_html(
    label: &str,
    token: Option<&str>,
    url: Option<&str>,
    action: Option<&crate::forms::Action>,
    appearance: Option<&crate::markup::Appearance>,
) -> String {
    let html = button_html(label, token, url, action);
    let Some(appearance) = appearance else {
        return html;
    };
    let mut attrs = String::new();
    match appearance.style {
        crate::markup::Style::Primary => attrs.push_str(" style=\"primary\""),
        crate::markup::Style::Success => attrs.push_str(" style=\"success\""),
        crate::markup::Style::Danger => attrs.push_str(" style=\"danger\""),
        crate::markup::Style::Link => attrs.push_str(" style=\"link\""),
        _ => {}
    }
    let html = html.replacen("<tg-button", &format!("<tg-button{attrs}"), 1);
    if let Some(id) = appearance.emoji_id {
        html.replacen(
            '>',
            &format!("><tg-emoji emoji-id=\"{id}\">▫</tg-emoji> "),
            1,
        )
    } else {
        html
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builder_escapes_untrusted_content() {
        let mut screen = RichScreen::new();
        screen
            .heading("<unsafe>")
            .table(&[vec!["a&b".into(), "x".into()]])
            .details("Title", "body");
        let button = screen
            .tap(Key {
                text: "click".into(),
                action: Some(crate::forms::Action::Close),
                url: None,
            })
            .unwrap();
        screen.html(&button, "click");
        let screen = screen.finish();
        screen.validate().unwrap();
        let rich = screen.rich_html.unwrap();
        assert!(rich.contains("&lt;unsafe&gt;"));
        assert!(rich.contains("a&amp;b"));
        assert!(rich.contains("{{atarustia:tap:0}}"));
    }
}
