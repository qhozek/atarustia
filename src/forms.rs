//! Expiring inline units. Callback data is opaque and never contains code, secrets or permissions.
use anyhow::{Context, Result, ensure};
use grammers_client::message::{Button, InputMessage, ReplyMarkup};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    Page {
        page: usize,
    },
    Close,
    Copy {
        text: String,
    },
    SwitchInline {
        query: String,
        #[serde(default)]
        same_chat: bool,
    },
    Disabled,
    Module {
        module: String,
        function: String,
        #[serde(default)]
        args: serde_json::Value,
    },
    ConfigToggle {
        module: String,
        key: String,
    },
    ConfigStep {
        module: String,
        key: String,
        step: i64,
    },
    ConfigReset {
        module: String,
        key: String,
    },
    ConfigOpen {
        module: String,
    },
    ConfigSet {
        module: String,
        key: String,
        value: serde_json::Value,
    },
    ConfigInput {
        module: String,
        key: String,
    },
    Help {
        module: Option<String>,
    },
    Update {
        current: String,
        incoming: String,
        #[serde(default)]
        retry: bool,
    },
    Restart,
    UpdateDismiss,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub text: String,
    #[serde(default)]
    pub action: Option<Action>,
    #[serde(default)]
    pub url: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Screen {
    pub text: String,
    pub pages: Vec<String>,
    pub buttons: Vec<Vec<Key>>,
    pub html: bool,
    pub rich_html: Option<String>,
    pub ttl_seconds: u64,
    pub media: Option<crate::media::Media>,
    pub gallery: Vec<crate::media::Media>,
    pub rich_buttons: Vec<Key>,
    pub button_appearance: Vec<Vec<crate::markup::Appearance>>,
    pub rich_button_appearance: Vec<crate::markup::Appearance>,
    pub close_button: bool,
    pub link_preview: bool,
}
impl Default for Screen {
    fn default() -> Self {
        Self {
            text: String::new(),
            pages: vec![],
            buttons: vec![],
            html: false,
            rich_html: None,
            ttl_seconds: 86400,
            media: None,
            gallery: vec![],
            rich_buttons: vec![],
            button_appearance: vec![],
            rich_button_appearance: vec![],
            close_button: true,
            link_preview: true,
        }
    }
}
#[derive(Clone)]
struct Unit {
    screen: Screen,
    owner: i64,
    allow: Vec<i64>,
    expires: Instant,
    tokens: Vec<String>,
    gate: Option<Gate>,
    page: usize,
    rich_render: Option<String>,
    rich_failed: bool,
}
#[derive(Clone)]
pub struct Gate {
    pub message: grammers_client::message::Message,
    pub command: String,
    pub module: Option<String>,
    pub permission: crate::security::Permission,
}
#[derive(Clone)]
pub struct Click {
    pub unit: String,
    pub action: Action,
    pub owner: i64,
}
#[derive(Default)]
pub struct Forms {
    units: HashMap<String, Unit>,
    callbacks: HashMap<String, (String, Action)>,
    inputs: HashMap<(i64, i32), Prompt>,
}
#[derive(Clone)]
pub struct Prompt {
    pub unit: String,
    pub user: i64,
    pub module: String,
    pub key: String,
    pub expires: Instant,
}
impl Screen {
    pub fn restrict_module(&self, module: &str) -> Result<()> {
        for key in self
            .buttons
            .iter()
            .flatten()
            .chain(self.rich_buttons.iter())
        {
            if let Some(action) = &key.action {
                ensure!(
                    match action {
                        Action::Page { .. }
                        | Action::Close
                        | Action::Copy { .. }
                        | Action::SwitchInline { .. }
                        | Action::Disabled => true,
                        Action::Module { module: m, .. } => m == module,
                        _ => false,
                    },
                    "module form may only call its own handlers"
                );
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.button_appearance.is_empty()
                || (self.button_appearance.len() == self.buttons.len()
                    && self
                        .button_appearance
                        .iter()
                        .zip(&self.buttons)
                        .all(|(a, b)| a.len() == b.len())),
            "button appearance must match keyboard shape"
        );
        ensure!(
            self.rich_button_appearance.is_empty()
                || self.rich_button_appearance.len() == self.rich_buttons.len(),
            "rich button appearance must match buttons"
        );
        for appearance in self
            .button_appearance
            .iter()
            .flatten()
            .chain(self.rich_button_appearance.iter())
        {
            appearance.validate()?;
        }
        if let Some(source) = &self.rich_html {
            for i in 0..self.rich_buttons.len() {
                ensure!(
                    source.contains(&crate::rich::marker(i)),
                    "rich button has no source marker"
                );
            }
            let mut rest = source.as_str();
            while let Some(start) = rest.find("{{atarustia:tap:") {
                rest = &rest[start + "{{atarustia:tap:".len()..];
                let end = rest.find("}}").context("unclosed rich button marker")?;
                let index = rest[..end]
                    .parse::<usize>()
                    .context("invalid rich button marker")?;
                ensure!(
                    index < self.rich_buttons.len(),
                    "rich button marker outside button list"
                );
                rest = &rest[end + 2..];
            }
        }
        ensure!(
            self.rich_buttons.is_empty() || self.rich_html.is_some(),
            "rich buttons need rich HTML"
        );
        ensure!(
            self.media.is_none() || self.gallery.is_empty(),
            "choose media or gallery, not both"
        );
        ensure!(self.gallery.len() <= 50, "gallery exceeds 50 items");
        let media_items: Vec<_> = self.media.iter().chain(self.gallery.iter()).collect();
        for media in &media_items {
            media.validate()?;
            ensure!(
                !matches!(media.source, crate::media::Source::Bytes { .. }),
                "form media requires HTTPS URL or cached file_id"
            );
        }
        if !media_items.is_empty() {
            ensure!(
                self.rich_html.is_none(),
                "rich HTML cannot be combined with media"
            );
            ensure!(
                self.text.encode_utf16().count() <= 1024
                    && self.pages.iter().all(|p| p.encode_utf16().count() <= 1024),
                "media captions exceed 1024 UTF-16 units"
            );
        }
        if !self.gallery.is_empty() {
            ensure!(
                self.pages.is_empty() || self.pages.len() == self.gallery.len(),
                "gallery captions must have one page per item"
            );
        }
        ensure!(
            self.rich_html
                .as_ref()
                .is_none_or(|s| s.len() <= 128 * 1024),
            "rich screen too large"
        );
        ensure!(
            (1..=86400).contains(&self.ttl_seconds),
            "form TTL must be 1–86400 seconds"
        );
        ensure!(
            self.pages.len() <= 50 && self.buttons.len() <= 100,
            "form limit exceeded"
        );
        ensure!(
            self.text.len() <= 128 * 1024
                && self.pages.iter().all(|s| if self.html {
                    let value = crate::bot_requests::Text {
                        text: s.clone(),
                        parse_mode: crate::bot_requests::ParseMode::Html,
                        entities: None,
                    };
                    std::panic::catch_unwind(|| value.parse(crate::output::MESSAGE_LIMIT, false))
                        .is_ok_and(|result| result.is_ok())
                } else {
                    s.encode_utf16().count() <= crate::output::MESSAGE_LIMIT
                }),
            "form text too large"
        );
        ensure!(
            self.buttons.iter().all(|r| r.len() <= 8)
                && self.buttons.iter().map(Vec::len).sum::<usize>() + self.rich_buttons.len() <= 90,
            "keyboard exceeds 90 buttons or 8 per row"
        );
        for key in self
            .buttons
            .iter()
            .flatten()
            .chain(self.rich_buttons.iter())
        {
            ensure!(
                !key.text.is_empty() && key.text.chars().count() <= 64,
                "button label too long"
            );
            ensure!(
                key.action.is_some() ^ key.url.is_some(),
                "button needs exactly one action or URL"
            );
            match &key.action {
                Some(Action::Copy { text }) => ensure!(
                    !text.is_empty() && text.chars().count() <= 256,
                    "copy text must contain 1–256 characters"
                ),
                Some(Action::SwitchInline { query, .. }) => {
                    ensure!(query.len() <= 1024, "inline query too long")
                }
                _ => {}
            }
            if let Some(url) = &key.url {
                let url = reqwest::Url::parse(url)?;
                ensure!(
                    matches!(url.scheme(), "https" | "tg"),
                    "button URL must be HTTPS or tg"
                );
            }
        }
        Ok(())
    }
}
impl Forms {
    pub fn clear(&mut self) {
        self.units.clear();
        self.callbacks.clear();
        self.inputs.clear();
    }
    pub fn clean(&mut self) {
        let now = Instant::now();
        let expired: Vec<_> = self
            .units
            .iter()
            .filter(|(_, u)| u.expires <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.remove(&id);
        }
        self.inputs
            .retain(|_, p| p.expires > now && self.units.contains_key(&p.unit));
    }
    pub fn create(
        &mut self,
        screen: Screen,
        owner: i64,
        allow: Vec<i64>,
    ) -> Result<(String, InputMessage)> {
        screen.validate()?;
        self.clean();
        ensure!(self.units.len() < 512, "too many active forms");
        let id = crate::web::random_token()[..24].to_owned();
        self.units.insert(
            id.clone(),
            Unit {
                expires: Instant::now() + Duration::from_secs(screen.ttl_seconds),
                screen,
                owner,
                allow,
                tokens: vec![],
                gate: None,
                page: 0,
                rich_render: None,
                rich_failed: false,
            },
        );
        let message = match self.render(&id, 0) {
            Ok(message) => message,
            Err(error) => {
                self.remove(&id);
                return Err(error);
            }
        };
        Ok((id, message))
    }
    pub fn set_gate(&mut self, id: &str, gate: Gate) -> Result<()> {
        self.units.get_mut(id).context("form expired")?.gate = Some(gate);
        Ok(())
    }
    pub fn gate(&self, id: &str) -> Option<Gate> {
        self.units.get(id).and_then(|u| u.gate.clone())
    }
    pub fn remove(&mut self, id: &str) -> bool {
        self.inputs.retain(|_, p| p.unit != id);
        if let Some(unit) = self.units.remove(id) {
            for token in unit.tokens {
                self.callbacks.remove(&token);
            }
            true
        } else {
            false
        }
    }
    pub fn input(&mut self, chat: i64, message: i32, prompt: Prompt) -> Result<()> {
        self.clean();
        ensure!(self.units.contains_key(&prompt.unit), "form expired");
        self.inputs.insert((chat, message), prompt);
        Ok(())
    }
    pub fn take_input(&mut self, chat: i64, reply: i32, user: i64) -> Option<Prompt> {
        self.clean();
        if self
            .inputs
            .get(&(chat, reply))
            .is_some_and(|p| p.user == user)
        {
            self.inputs.remove(&(chat, reply))
        } else {
            None
        }
    }
    pub fn click(&mut self, data: &str, user: i64) -> Result<Click> {
        self.clean();
        let (id, action) = self
            .callbacks
            .get(data)
            .cloned()
            .context("form expired or button replaced")?;
        let unit = self.units.get(&id).context("form expired")?;
        ensure!(
            user == unit.owner || unit.allow.contains(&user),
            "button is not authorized for this user"
        );
        Ok(Click {
            unit: id,
            action,
            owner: unit.owner,
        })
    }
    pub fn replace(&mut self, id: &str, screen: Screen) -> Result<InputMessage> {
        screen.validate()?;
        let unit = self.units.get_mut(id).context("form expired")?;
        unit.screen = screen;
        unit.rich_render = None;
        self.render(id, 0)
    }
    pub fn rich_source(&self, id: &str) -> Option<String> {
        self.units
            .get(id)
            .filter(|u| !u.rich_failed)
            .and_then(|u| u.rich_render.clone().or_else(|| u.screen.rich_html.clone()))
    }
    pub fn media(&self, id: &str) -> Option<crate::media::Media> {
        self.units.get(id).and_then(|u| {
            u.screen
                .media
                .clone()
                .or_else(|| u.screen.gallery.get(u.page).cloned())
        })
    }
    pub fn inline_result(
        &mut self,
        id: &str,
    ) -> Result<grammers_client::tl::enums::InputBotInlineResult> {
        let body = self.render(id, 0)?;
        crate::media::inline_result(
            id,
            "atarustia",
            body,
            self.media(id).as_ref(),
            self.rich_source(id).as_deref(),
        )
    }
    pub fn plain_fallback(&mut self, id: &str) -> bool {
        if let Some(unit) = self.units.get_mut(id) {
            if unit.screen.rich_html.is_some() && !unit.rich_failed {
                unit.rich_failed = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    }
    pub fn restrict_module(&self, id: &str, module: &str) -> Result<()> {
        let unit = self.units.get(id).context("form expired")?;
        unit.screen.restrict_module(module)
    }
    pub fn invalidate_module(&mut self, module: &str) {
        let ids: Vec<_> = self
            .units
            .iter()
            .filter(|(_, u)| {
                u.gate.as_ref().and_then(|g| g.module.as_deref()) == Some(module)
                    || u.screen.buttons.iter().flatten().chain(u.screen.rich_buttons.iter()).any(
                        |key| matches!(&key.action,Some(Action::Module{module:m,..}) if m==module),
                    )
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.remove(&id);
        }
    }
    pub fn render(&mut self, id: &str, page: usize) -> Result<InputMessage> {
        self.clean();
        let unit = self.units.get_mut(id).context("form expired")?;
        for token in unit.tokens.drain(..) {
            self.callbacks.remove(&token);
        }
        let pages = if !unit.screen.gallery.is_empty() && unit.screen.pages.is_empty() {
            vec![unit.screen.text.clone(); unit.screen.gallery.len()]
        } else if unit.screen.pages.is_empty() {
            crate::output::pages(&unit.screen.text)
        } else {
            unit.screen.pages.clone()
        };
        let page = page.min(pages.len().saturating_sub(1));
        unit.page = page;
        let mut rows = unit.screen.buttons.clone();
        let mut appearances = if unit.screen.button_appearance.is_empty() {
            unit.screen
                .buttons
                .iter()
                .map(|r| vec![crate::markup::Appearance::default(); r.len()])
                .collect::<Vec<_>>()
        } else {
            unit.screen.button_appearance.clone()
        };
        if unit.rich_failed {
            rows.extend(unit.screen.rich_buttons.chunks(2).map(|r| r.to_vec()));
            let rich_styles = if unit.screen.rich_button_appearance.is_empty() {
                vec![crate::markup::Appearance::default(); unit.screen.rich_buttons.len()]
            } else {
                unit.screen.rich_button_appearance.clone()
            };
            appearances.extend(rich_styles.chunks(2).map(|r| r.to_vec()));
            unit.rich_render = None;
        } else if let Some(source) = &unit.screen.rich_html {
            let mut rich = source.clone();
            for (index, key) in unit.screen.rich_buttons.iter().enumerate() {
                let token = if let Some(action) = &key.action {
                    if matches!(
                        action,
                        Action::Page { .. }
                            | Action::Close
                            | Action::Module { .. }
                            | Action::Help { .. }
                            | Action::ConfigOpen { .. }
                            | Action::ConfigToggle { .. }
                            | Action::ConfigStep { .. }
                            | Action::ConfigReset { .. }
                            | Action::ConfigSet { .. }
                            | Action::ConfigInput { .. }
                    ) {
                        let token = format!("f:{}", &crate::web::random_token()[..32]);
                        self.callbacks
                            .insert(token.clone(), (id.into(), action.clone()));
                        unit.tokens.push(token.clone());
                        Some(token)
                    } else {
                        None
                    }
                } else {
                    None
                };
                rich = rich.replace(
                    &crate::rich::marker(index),
                    &crate::rich::styled_button_html(
                        &key.text,
                        token.as_deref(),
                        key.url.as_deref(),
                        key.action.as_ref(),
                        unit.screen.rich_button_appearance.get(index),
                    ),
                );
            }
            unit.rich_render = Some(rich);
        }
        if pages.len() > 1 {
            let mut nav = vec![];
            if page > 0 {
                nav.push(Key {
                    text: "‹".into(),
                    action: Some(Action::Page { page: page - 1 }),
                    url: None,
                });
            }
            nav.push(Key {
                text: format!("{} / {}", page + 1, pages.len()),
                action: Some(Action::Page { page }),
                url: None,
            });
            if page + 1 < pages.len() {
                nav.push(Key {
                    text: "›".into(),
                    action: Some(Action::Page { page: page + 1 }),
                    url: None,
                });
            }
            rows.push(nav);
        }
        if unit.screen.close_button {
            rows.push(vec![Key {
                text: "✕".into(),
                action: Some(Action::Close),
                url: None,
            }]);
        }
        let mut keyboard = vec![];
        for (row_index, row) in rows.into_iter().enumerate() {
            let mut buttons = vec![];
            for key in row {
                if let Some(url) = key.url {
                    buttons.push(Button::url(key.text, url));
                } else if let Some(action) = key.action {
                    match action {
                        Action::Copy { text } => {
                            buttons.push(Button {
                                raw: grammers_client::tl::types::KeyboardInlineButton {
                                    text: key.text,
                                    r#type: grammers_client::tl::types::InlineButtonTypeCopy {
                                        copy_text: text,
                                    }
                                    .into(),
                                    style: None,
                                }
                                .into(),
                            });
                            continue;
                        }
                        Action::SwitchInline { query, same_chat } => {
                            buttons.push(if same_chat {
                                Button::switch(key.text, query)
                            } else {
                                Button::switch_elsewhere(key.text, query)
                            });
                            continue;
                        }
                        Action::Disabled => {
                            buttons.push(Button {
                                raw: grammers_client::tl::types::KeyboardInlineButton {
                                    text: key.text,
                                    r#type: grammers_client::tl::types::InlineButtonTypeCallback {
                                        data: vec![0],
                                        requires_password: false,
                                    }
                                    .into(),
                                    style: None,
                                }
                                .into(),
                            });
                            continue;
                        }
                        _ => {}
                    }
                    let token = format!("f:{}", &crate::web::random_token()[..32]);
                    self.callbacks
                        .insert(token.clone(), (id.to_owned(), action));
                    unit.tokens.push(token.clone());
                    buttons.push(Button::data(key.text, token.into_bytes()));
                }
            }
            keyboard.push(buttons);
            if let Some(styles) = appearances.get(row_index) {
                for (button, style) in keyboard.last_mut().unwrap().iter_mut().zip(styles) {
                    style.apply(button)?;
                }
            }
        }
        let body = pages.get(page).cloned().unwrap_or_default();
        let body = if body.is_empty() {
            "\u{2060}".into()
        } else {
            body
        };
        let message = if let Some(rich) = unit.rich_render.as_ref().filter(|_| !unit.rich_failed) {
            InputMessage::new().text(body).rich_message(
                grammers_client::message::InputRichMessage::html(rich.clone()),
            )
        } else if unit.screen.html {
            InputMessage::new().html(body)
        } else {
            InputMessage::new().text(body)
        };
        Ok(message
            .link_preview(unit.screen.link_preview)
            .reply_markup(ReplyMarkup::from_buttons(&keyboard)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rich_taps_are_bound_and_plain_fallback_preserves_action() {
        let mut builder = crate::rich::RichScreen::new();
        let tap = builder
            .tap(Key {
                text: "Go".into(),
                action: Some(Action::Close),
                url: None,
            })
            .unwrap();
        builder.html(&tap, "Go");
        let mut forms = Forms::default();
        let (id, _) = forms.create(builder.finish(), 1, vec![]).unwrap();
        let rich = forms.rich_source(&id).unwrap();
        assert!(!rich.contains("{{atarustia:"));
        let token = forms.units[&id].tokens[0].clone();
        assert!(rich.contains(&token));
        assert!(forms.click(&token, 1).is_ok());
        assert!(forms.plain_fallback(&id));
        forms.render(&id, 0).unwrap();
        assert!(forms.rich_source(&id).is_none());
        assert!(forms.click(&token, 1).is_err());
        assert!(forms.units[&id].tokens.len() >= 2);
    }
    #[test]
    fn gallery_uses_media_results_and_limits() {
        let mut forms = Forms::default();
        let screen = Screen {
            text: "caption".into(),
            gallery: vec![
                crate::media::Media::from_url(
                    crate::file_id::FileKind::Photo,
                    "https://example.com/1.jpg",
                ),
                crate::media::Media::from_url(
                    crate::file_id::FileKind::Video,
                    "https://example.com/2.mp4",
                ),
            ],
            ..Default::default()
        };
        let (id, _) = forms.create(screen, 1, vec![]).unwrap();
        let result = forms.inline_result(&id).unwrap();
        assert!(
            matches!(result,grammers_client::tl::enums::InputBotInlineResult::Result(r) if r.r#type=="photo")
        );
        forms.render(&id, 1).unwrap();
        assert_eq!(
            forms.media(&id).unwrap().kind,
            crate::file_id::FileKind::Video
        );
        let invalid = Screen {
            gallery: vec![crate::media::Media::from_url(
                crate::file_id::FileKind::Photo,
                "https://example.com/a.jpg",
            )],
            rich_html: Some("<h1>rich</h1>".into()),
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn closed_module_forms_and_privileged_actions() {
        let mut forms = Forms::default();
        let screen = Screen {
            text: "counter".into(),
            buttons: vec![vec![Key {
                text: "click".into(),
                action: Some(Action::Module {
                    module: "counter".into(),
                    function: "increment".into(),
                    args: serde_json::Value::Null,
                }),
                url: None,
            }]],
            ..Default::default()
        };
        screen.restrict_module("counter").unwrap();
        assert!(screen.restrict_module("other").is_err());
        let (id, _) = forms.create(screen, 1, vec![]).unwrap();
        forms.invalidate_module("counter");
        assert!(!forms.units.contains_key(&id));
        let privileged = Screen {
            buttons: vec![vec![Key {
                text: "change".into(),
                action: Some(Action::ConfigOpen {
                    module: "counter".into(),
                }),
                url: None,
            }]],
            ..Default::default()
        };
        assert!(privileged.restrict_module("counter").is_err());
    }
    #[test]
    fn ownership_expiry_and_replacement() {
        let mut f = Forms::default();
        let (id, _) = f
            .create(
                Screen {
                    text: "hi".into(),
                    ..Default::default()
                },
                1,
                vec![2],
            )
            .unwrap();
        let token = f.units[&id].tokens[0].clone();
        assert!(f.click(&token, 3).is_err());
        assert!(f.click(&token, 2).is_ok());
        f.render(&id, 0).unwrap();
        assert!(f.click(&token, 1).is_err());
        let token = f.units[&id].tokens[0].clone();
        f.units.get_mut(&id).unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert!(f.click(&token, 1).is_err());
        assert!(f.callbacks.is_empty());
    }
    #[test]
    fn invalid_url_and_keyboard() {
        assert!(
            Screen {
                buttons: vec![vec![Key {
                    text: "bad".into(),
                    url: Some("javascript:alert(1)".into()),
                    action: None
                }]],
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn reply_inputs_are_scoped_to_user_and_chat() {
        let mut forms = Forms::default();
        let (id, _) = forms
            .create(
                Screen {
                    text: "settings".into(),
                    ..Default::default()
                },
                1,
                vec![],
            )
            .unwrap();
        forms
            .input(
                1,
                10,
                Prompt {
                    unit: id.clone(),
                    user: 1,
                    module: "hello".into(),
                    key: "greeting".into(),
                    expires: Instant::now() + Duration::from_secs(10),
                },
            )
            .unwrap();
        assert!(forms.take_input(2, 10, 1).is_none());
        assert!(forms.take_input(1, 10, 2).is_none());
        assert!(forms.take_input(1, 10, 1).is_some());
        assert!(forms.take_input(1, 10, 1).is_none());
        forms.remove(&id);
        assert!(forms.inputs.is_empty());
    }
}
