//! Ataraxia configurator: pure screens/state transitions, with storage and Telegram adapters below.
use crate::{
    forms::{Action, Key, Screen},
    markup::{Appearance, Style},
    output::escape,
    validators::Validator,
};
use anyhow::{Context as _, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub module: Option<String>,
    pub parameter: Option<String>,
    pub folder: Option<String>,
    pub show: bool,
    pub confirm_reset: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum Input {
    Value,
    Add,
    Remove,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Operation {
    Navigate,
    Set { value: Value },
    Toggle { value: Value },
    Step { step: f64, integer: bool },
    RemoveItem { index: usize, item: Value },
    Reset,
    ResetAll,
    Input { mode: Input },
}
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub kind: &'static str,
    pub values: Value,
    pub defaults: Value,
    pub validators: BTreeMap<String, Validator>,
    pub docs: BTreeMap<String, String>,
    pub folders: BTreeMap<String, String>,
}
impl Module {
    fn changed(&self, key: &str) -> bool {
        self.values.get(key) != self.defaults.get(key)
    }
    fn changes(&self) -> usize {
        self.keys().filter(|k| self.changed(k)).count()
    }
    fn keys(&self) -> impl Iterator<Item = &String> {
        self.defaults.as_object().into_iter().flat_map(|m| m.keys())
    }
}
#[derive(Clone, Copy)]
pub struct Ui {
    pub english: bool,
    pub premium: bool,
}
pub fn format(template: &str, values: &[String]) -> String {
    let mut out = String::new();
    let mut rest = template;
    for value in values {
        let Some((head, tail)) = rest.split_once("{}") else {
            break;
        };
        out.push_str(head);
        out.push_str(value);
        rest = tail;
    }
    out.push_str(rest);
    out
}
impl Ui {
    pub fn text(self, key: &str) -> String {
        crate::original::text(self.english, "ataraxia_config", key)
    }
    fn f(self, key: &str, values: &[String]) -> String {
        format(&self.text(key), values)
    }
    fn plural(self, n: usize, key: &str) -> String {
        let text = self.text(key);
        let forms: Vec<_> = text.split('|').collect();
        let index = if forms.len() == 3 {
            if (11..=14).contains(&(n % 100)) {
                2
            } else if n % 10 == 1 {
                0
            } else if (2..=4).contains(&(n % 10)) {
                1
            } else {
                2
            }
        } else if n == 1 {
            0
        } else {
            forms.len() - 1
        };
        format(forms[index], &[n.to_string()])
    }
    fn icon(self, name: &str) -> String {
        let (id, emoji, plain) = match name {
            "settings" => (6021582331251268218i64, "⚙", "⚙"),
            "core" => (6023761060786346622, "⚡️", "⛈"),
            "external" => (6024106569430472546, "📦", "📦"),
            "library" => (6021836189588265800, "📖", "📚"),
            "module" => (6024065724291488135, "🧩", "🧩"),
            "edit" => (6024110353296660793, "✏️", "✎"),
            "folder" => (6021375494216226506, "📁", "📁"),
            "info" => (6019094432790354513, "💬", "ℹ️"),
            "warning" => (6019102674832595118, "⚠️", "⚠"),
            _ => (0, "", ""),
        };
        if self.premium && id != 0 {
            format!("<tg-emoji emoji-id=\"{id}\">{emoji}</tg-emoji>")
        } else {
            plain.into()
        }
    }
    fn value(self, value: &Value, kind: &str, hidden: bool, limit: usize) -> String {
        if hidden {
            return "<code>••••••</code>".into();
        }
        if kind == "Boolean" || value.is_boolean() {
            return format!(
                "{} {}",
                if value.as_bool().unwrap_or(false) {
                    "●"
                } else {
                    "○"
                },
                self.text(if value.as_bool().unwrap_or(false) {
                    "on_short"
                } else {
                    "off_short"
                })
            );
        }
        if let Some(a) = value.as_array() {
            return escape(&self.plural(a.len(), "items_count"));
        }
        if value.is_null() || value.as_str() == Some("") {
            return format!("<i>{}</i>", self.text("empty"));
        }
        format!("<code>{}</code>", escape(&short(&python(value), limit)))
    }
}
pub fn python(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|v| if let Some(s) = v.as_str() {
                    format!("'{s}'")
                } else {
                    python(v)
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => value.to_string(),
    }
}
fn short(s: &str, limit: usize) -> String {
    if s.chars().count() <= limit {
        s.into()
    } else {
        s.chars()
            .take(limit.saturating_sub(1))
            .chain(['…'])
            .collect()
    }
}
fn inner(v: Option<&Validator>) -> (Option<&Validator>, bool) {
    match v {
        Some(Validator::Hidden { inner }) => (Some(inner), true),
        _ => (v, false),
    }
}
pub fn kind(v: Option<&Validator>) -> &'static str {
    match v {
        Some(Validator::Boolean) => "Boolean",
        Some(Validator::Integer { .. }) => "Integer",
        Some(Validator::Float { .. }) => "Float",
        Some(Validator::String { .. }) => "String",
        Some(Validator::Choice { .. }) => "Choice",
        Some(Validator::MultiChoice { .. }) => "MultiChoice",
        Some(Validator::Series { .. }) => "Series",
        Some(Validator::Link) => "Link",
        Some(Validator::TelegramId) => "TelegramID",
        Some(Validator::Union { .. }) => "Union",
        Some(Validator::None) => "NoneType",
        Some(Validator::Hidden { inner }) => kind(Some(inner)),
        None => "",
    }
}
fn action(state: State, operation: Operation) -> Action {
    Action::Configure { state, operation }
}
fn key(text: impl Into<String>, action: Action) -> Key {
    Key {
        text: text.into(),
        action: Some(action),
        url: None,
    }
}
fn nav(text: impl Into<String>, state: State) -> Key {
    key(text, action(state, Operation::Navigate))
}
fn badge(text: impl AsRef<str>, style: &str) -> String {
    format!(
        "<tg-button type=\"disabled\"{}>{}</tg-button>",
        if style.is_empty() {
            String::new()
        } else {
            format!(" style=\"{style}\"")
        },
        escape(text.as_ref())
    )
}
fn details(title: &str, body: &str) -> String {
    format!("<details><summary>{title}</summary>{body}</details>")
}
fn row(buttons: &[String]) -> String {
    format!(
        "<tg-button-row align=\"left\">{}</tg-button-row>",
        buttons.join("")
    )
}
fn plain(source: &str) -> String {
    let mut s = source.replace("<br>", "\n");
    for tag in ["</summary>", "</details>", "</tr>", "</tg-button-row>"] {
        s = s.replace(tag, "\n");
    }
    s = s.replace("</blockquote>", "</blockquote>\n");
    s = s.replace("</th>", ": ").replace("</td><td>", " · ");
    let re =
        regex::Regex::new(r"</?(?:details|summary|table|tr|th|td|tg-button-row|tg-button)\b[^>]*>")
            .unwrap();
    re.replace_all(&s, "").into_owned()
}
struct Canvas {
    ui: Ui,
    source: Vec<String>,
    taps: Vec<Key>,
    styles: Vec<Appearance>,
    rows: Vec<Vec<Key>>,
    row_styles: Vec<Vec<Appearance>>,
}
impl Canvas {
    fn new(ui: Ui) -> Self {
        Self {
            ui,
            source: vec![],
            taps: vec![],
            styles: vec![],
            rows: vec![],
            row_styles: vec![],
        }
    }
    fn add(&mut self, s: impl Into<String>) {
        self.source.push(s.into());
    }
    fn tap(
        &mut self,
        label: impl Into<String>,
        state: State,
        operation: Operation,
        style: Style,
    ) -> String {
        let marker = crate::rich::marker(self.taps.len());
        self.taps.push(key(label, action(state, operation)));
        self.styles.push(Appearance {
            style,
            emoji_id: None,
        });
        marker
    }
    fn keyboard(&mut self, row: Vec<Key>) {
        if !row.is_empty() {
            let (row, styles): (Vec<_>, Vec<_>) = row
                .into_iter()
                .map(|mut key| {
                    let icon = [
                        ("‹ ", 5807619215321996754i64),
                        ("✕ ", 5807692706507399432),
                        ("✎ ", 6024110353296660793),
                        ("📋 ", 6021445064096486884),
                        ("↺ ", 5807900819147724161),
                        ("➕ ", 5807642902066634351),
                    ]
                    .into_iter()
                    .find(|(prefix, _)| key.text.starts_with(prefix));
                    let mut appearance = Appearance::default();
                    if self.ui.premium
                        && let Some((prefix, id)) = icon
                    {
                        key.text = key.text[prefix.len()..].into();
                        appearance.emoji_id = Some(id);
                    }
                    (key, appearance)
                })
                .unzip();
            self.row_styles.push(styles);
            self.rows.push(row);
        }
    }
    fn footer(&mut self, back: Option<Key>) {
        let close = key(
            format!(
                "✕ {}",
                crate::original::text(self.ui.english, "$inline", "screen_close")
            ),
            Action::Close,
        );
        self.keyboard(back.into_iter().chain([close]).collect());
    }
    fn finish(self, reply: Option<State>) -> Screen {
        let source = self.source.join("\n");
        let (text, fallback) = fallback(&source, &self.taps);
        Screen {
            text,
            rich_html: Some(to_rich(&source)),
            rich_buttons: self.taps,
            rich_button_appearance: self.styles,
            buttons: self.rows,
            button_appearance: self.row_styles,
            html: true,
            close_button: false,
            link_preview: false,
            config_reply: reply,
            config_fallback: Some(fallback),
            ..Default::default()
        }
    }
}
fn to_rich(source: &str) -> String {
    let blocks=regex::Regex::new(r"\n*(</?(?:details|summary|table|tr|th|td|blockquote|tg-button-row|img|h[1-6]|hr|pre|ul|ol|li)\b[^>]*>)\n*").unwrap();
    let source = blocks.replace_all(source.trim_matches('\n'), "$1");
    let pre = regex::Regex::new(r"(?s)<pre\b[^>]*>.*?</pre>").unwrap();
    let mut result = String::new();
    let mut offset = 0;
    for found in pre.find_iter(&source) {
        result.push_str(&source[offset..found.start()].replace('\n', "<br>"));
        result.push_str(found.as_str());
        offset = found.end();
    }
    result.push_str(&source[offset..].replace('\n', "<br>"));
    result
}
fn fallback(source: &str, taps: &[Key]) -> (String, Vec<Vec<Key>>) {
    let row_re = regex::Regex::new(r#"(?s)<tg-button-row\b[^>]*>(.*?)</tg-button-row>"#).unwrap();
    let marker_re = regex::Regex::new(r"\{\{atarustia:tap:(\d+)\}\}").unwrap();
    let cell_re = regex::Regex::new(
        r#"\{\{atarustia:tap:(\d+)\}\}|<tg-button type="disabled"[^>]*>(.*?)</tg-button>"#,
    )
    .unwrap();
    let mut rows = vec![];
    let mut rest = source;
    let mut text = String::new();
    let mut loose = vec![];
    let add_loose = |s: &str, loose: &mut Vec<Key>| {
        for capture in marker_re.captures_iter(s) {
            if let Ok(i) = capture[1].parse::<usize>()
                && let Some(k) = taps.get(i)
            {
                loose.push(k.clone());
            }
        }
    };
    for found in row_re.captures_iter(source) {
        let whole = found.get(0).unwrap();
        let before = &rest[..whole.start() - (source.len() - rest.len())];
        text.push_str(before);
        add_loose(before, &mut loose);
        rows.extend(loose.chunks(2).map(|r| r.to_vec()));
        loose.clear();
        let mut keys = vec![];
        for capture in cell_re.captures_iter(&found[1]) {
            if let Some(label) = capture.get(2) {
                keys.push(key(label.as_str(), Action::Disabled));
            } else if let Ok(i) = capture[1].parse::<usize>()
                && let Some(k) = taps.get(i)
            {
                keys.push(k.clone());
            }
        }
        rows.extend(keys.chunks(8).map(|r| r.to_vec()));
        rest = &source[whole.end()..];
    }
    text.push_str(rest);
    add_loose(rest, &mut loose);
    rows.extend(loose.chunks(2).map(|r| r.to_vec()));
    for (i, k) in taps.iter().enumerate() {
        text = text.replace(&crate::rich::marker(i), &escape(&k.text));
    }
    (plain(&text), rows)
}
fn back(ui: Ui, state: State) -> Key {
    nav(
        format!(
            "‹ {}",
            crate::original::text(ui.english, "$inline", "screen_back")
        ),
        state,
    )
}
fn selected<'a>(modules: &'a [Module], name: &str) -> Option<&'a Module> {
    modules.iter().find(|m| {
        m.name.eq_ignore_ascii_case(name)
            || format!("{}Mod", m.name).eq_ignore_ascii_case(name)
            || m.name == "Settings" && name.eq_ignore_ascii_case("CoreMod")
    })
}
fn module_state(state: &State, name: &str) -> State {
    State {
        module: Some(name.into()),
        folder: state.folder.clone(),
        ..Default::default()
    }
}
fn parameter_state(state: &State, name: &str, parameter: &str) -> State {
    State {
        parameter: Some(parameter.into()),
        ..module_state(state, name)
    }
}
fn entry(c: &mut Canvas, m: &Module) -> String {
    let tap = c.tap(
        &m.name,
        State {
            module: Some(m.name.clone()),
            ..Default::default()
        },
        Operation::Navigate,
        Style::Link,
    );
    format!(
        "{tap} {}{}",
        badge(m.keys().count().to_string(), ""),
        if m.changes() > 0 {
            format!(" {}{}", c.ui.icon("edit"), m.changes())
        } else {
            String::new()
        }
    )
}
pub fn render(modules: &[Module], state: &State, ui: Ui) -> Result<Screen> {
    let mut c = Canvas::new(ui);
    let Some(name) = &state.module else {
        if let Some(folder) = &state.folder {
            if !modules
                .iter()
                .any(|m| m.folders.values().any(|f| f == folder))
            {
                let mut screen = render(modules, &State::default(), ui)?;
                warn(
                    &mut screen,
                    &ui.f("folder_missing", std::slice::from_ref(folder)),
                );
                return Ok(screen);
            }
            c.add(format!("{} <b>{}</b>", ui.icon("folder"), escape(folder)));
            for m in modules {
                let params: Vec<_> = m
                    .keys()
                    .filter(|p| m.folders.get(*p) == Some(folder))
                    .collect();
                if params.is_empty() {
                    continue;
                }
                let tap = c.tap(
                    &m.name,
                    module_state(state, &m.name),
                    Operation::Navigate,
                    Style::Link,
                );
                c.add(format!("{} {tap}", ui.icon(m.kind)));
                for p in params {
                    let tap = c.tap(
                        p,
                        parameter_state(state, &m.name, p),
                        Operation::Navigate,
                        Style::Link,
                    );
                    let (v, h) = inner(m.validators.get(p));
                    c.add(format!(
                        "    {tap} {}",
                        ui.value(&m.values[p], kind(v), h, 32)
                    ));
                }
            }
            c.footer(Some(back(ui, State::default())));
            return Ok(c.finish(None));
        }
        let changes: usize = modules.iter().map(Module::changes).sum();
        c.add(format!(
            "{} <b>{}</b> {}{}",
            ui.icon("settings"),
            ui.text("title"),
            badge(ui.plural(modules.len(), "modules_count"), "primary"),
            if changes > 0 {
                format!(
                    " {}",
                    badge(ui.f("changed_count", &[changes.to_string()]), "")
                )
            } else {
                String::new()
            }
        ));
        for (kind, title) in [
            ("core", "builtin"),
            ("external", "external"),
            ("library", "libraries"),
        ] {
            let mut group: Vec<_> = modules.iter().filter(|m| m.kind == kind).collect();
            group.sort_by_key(|m| m.name.to_lowercase());
            if group.is_empty() {
                continue;
            }
            let entries = group
                .iter()
                .map(|m| entry(&mut c, m))
                .collect::<Vec<_>>()
                .join("\n");
            c.add(details(
                &format!(
                    "{} <b>{}</b> · {}",
                    ui.icon(kind),
                    ui.text(title),
                    group.len()
                ),
                &entries,
            ));
        }
        let mut folders = BTreeMap::<&str, usize>::new();
        for m in modules {
            for p in m.keys() {
                if let Some(f) = m.folders.get(p) {
                    *folders.entry(f).or_default() += 1;
                }
            }
        }
        if !folders.is_empty() {
            let mut entries = vec![];
            for (f, n) in &folders {
                let tap = c.tap(
                    *f,
                    State {
                        folder: Some((*f).into()),
                        ..Default::default()
                    },
                    Operation::Navigate,
                    Style::Link,
                );
                entries.push(format!("{tap} {}", badge(n.to_string(), "")));
            }
            c.add(details(
                &format!(
                    "{} <b>{}</b> · {}",
                    ui.icon("folder"),
                    ui.text("folders"),
                    folders.len()
                ),
                &entries.join("\n"),
            ));
        }
        if modules.is_empty() {
            c.add(format!("<i>{}</i>", ui.text("empty")));
        }
        c.footer(None);
        return Ok(c.finish(None));
    };
    let Some(m) = selected(modules, name) else {
        let mut screen = render(modules, &State::default(), ui)?;
        warn(
            &mut screen,
            &ui.f("mod_missing", std::slice::from_ref(name)),
        );
        return Ok(screen);
    };
    let state = &State {
        module: Some(m.name.clone()),
        ..state.clone()
    };
    if state.confirm_reset {
        c.add(format!(
            "{} {}",
            ui.icon("warning"),
            ui.f("reset_confirm", &[format!("<b>{}</b>", escape(&m.name))])
        ));
        c.keyboard(vec![
            key(
                format!("↺ {}", ui.text("reset_yes_btn")),
                action(module_state(state, &m.name), Operation::ResetAll),
            ),
            nav(ui.text("cancel_btn"), module_state(state, &m.name)),
        ]);
        c.row_styles[0][0].style = Style::Danger;
        return Ok(c.finish(None));
    }
    let Some(param) = &state.parameter else {
        c.add(format!(
            "{} <b>{}</b> › {}{}",
            ui.icon(if m.kind == "external" {
                "module"
            } else {
                m.kind
            }),
            escape(&m.name),
            ui.text("module_settings"),
            if m.changes() > 0 {
                format!(
                    " {}",
                    badge(ui.f("changed_count", &[m.changes().to_string()]), "")
                )
            } else {
                String::new()
            }
        ));
        let mut docs = vec![];
        for p in m.keys() {
            let tap = c.tap(
                p,
                parameter_state(state, &m.name, p),
                Operation::Navigate,
                Style::Link,
            );
            let (v, h) = inner(m.validators.get(p));
            c.add(format!(
                "{tap}{} {}",
                if m.changed(p) {
                    format!(" {}", ui.icon("edit"))
                } else {
                    String::new()
                },
                ui.value(&m.values[p], kind(v), h, 32)
            ));
            if let Some(d) = m.docs.get(p) {
                docs.push(format!("• <b>{}</b> — {}", escape(p), escape(d)));
            }
        }
        if !docs.is_empty() {
            c.add(details(
                &format!("{} {}", ui.icon("info"), ui.text("about")),
                &docs.join("\n"),
            ));
        }
        if m.changes() > 0 {
            c.keyboard(vec![nav(
                format!("↺ {}", ui.text("reset_all_btn")),
                State {
                    confirm_reset: true,
                    ..module_state(state, &m.name)
                },
            )]);
        }
        c.footer(Some(back(
            ui,
            State {
                folder: state.folder.clone(),
                ..Default::default()
            },
        )));
        return Ok(c.finish(None));
    };
    let Some(p) = m.keys().find(|p| p.eq_ignore_ascii_case(param)) else {
        let mut screen = render(modules, &module_state(state, &m.name), ui)?;
        warn(
            &mut screen,
            &ui.f("param_missing", &[m.name.clone(), param.clone()]),
        );
        return Ok(screen);
    };
    let state = &State {
        parameter: Some(p.clone()),
        ..state.clone()
    };
    let (value, default) = (&m.values[p], &m.defaults[p]);
    let (v, hidden) = inner(m.validators.get(p));
    let k = kind(v);
    let masked = hidden && !state.show;
    let tap = c.tap(
        &m.name,
        module_state(state, &m.name),
        Operation::Navigate,
        Style::Link,
    );
    c.add(format!(
        "{} {tap} › <b>{}</b>{}",
        ui.icon(if m.kind == "external" {
            "module"
        } else {
            m.kind
        }),
        escape(p),
        if m.changed(p) {
            format!(" {}", ui.icon("edit"))
        } else {
            String::new()
        }
    ));
    if let Some(doc) = m.docs.get(p) {
        c.add(format!("<blockquote>{}</blockquote>", escape(doc)));
    }
    let text_kind = matches!(
        k,
        "Boolean" | "Choice" | "MultiChoice" | "Integer" | "Float"
    );
    let long = !text_kind
        && k != "Series"
        && value
            .as_str()
            .is_some_and(|s| s.chars().count() > 64 || s.contains('\n'));
    let mut facts = vec![];
    if !long || masked {
        facts.push((ui.text("current"), ui.value(value, k, masked, 48)));
    }
    facts.push((ui.text("default"), ui.value(default, k, masked, 48)));
    let range = match v {
        Some(Validator::Integer { min, max }) => {
            bounds(min.map(|v| v as f64), max.map(|v| v as f64))
        }
        Some(Validator::Float { min, max }) => bounds(*min, *max),
        _ => String::new(),
    };
    facts.push((
        ui.text("type"),
        format!(
            "<code>{}</code>{}",
            if k.is_empty() {
                ui.text("any_type")
            } else {
                k.into()
            },
            if range.is_empty() {
                String::new()
            } else {
                format!(" · {range}")
            }
        ),
    ));
    c.add(format!(
        "<table bordered compact>{}</table>",
        facts
            .into_iter()
            .map(|(k, v)| format!("<tr><th>{k}</th><td>{v}</td></tr>"))
            .collect::<String>()
    ));
    if long && !masked {
        c.add(format!(
            "<b>{}</b>\n<blockquote expandable>{}</blockquote>\n{} <i>{}</i>",
            ui.text("current"),
            escape(value.as_str().unwrap()),
            ui.icon("info"),
            ui.text("long_hint")
        ));
    }
    if !text_kind && let Some(validator) = v {
        let doc = validator_doc(ui, validator);
        let hint = ui.f("typehint", std::slice::from_ref(&doc)).replace(
            "{eng_art}",
            if doc
                .to_lowercase()
                .starts_with(['e', 'u', 'i', 'o', 'a', 'y'])
            {
                "n"
            } else {
                ""
            },
        );
        c.add(format!("<i>{}</i>", escape(&hint)));
    }
    if hidden {
        let tap = c.tap(
            ui.text(if state.show { "hide_btn" } else { "show_btn" }),
            State {
                show: !state.show,
                ..state.clone()
            },
            Operation::Navigate,
            Style::Default,
        );
        c.add(row(&[tap]));
    }
    if !masked {
        match v {
            Some(Validator::Boolean) => {
                let on = value.as_bool().unwrap_or(false);
                let a = c.tap(
                    format!("{} {}", if on { "●" } else { "○" }, ui.text("on")),
                    state.clone(),
                    Operation::Set { value: json!(true) },
                    if on { Style::Success } else { Style::Default },
                );
                let b = c.tap(
                    format!("{} {}", if on { "○" } else { "●" }, ui.text("off")),
                    state.clone(),
                    Operation::Set {
                        value: json!(false),
                    },
                    if on { Style::Default } else { Style::Danger },
                );
                c.add(row(&[a, b]));
            }
            Some(Validator::Choice { values }) | Some(Validator::MultiChoice { values }) => {
                let multi = k == "MultiChoice";
                let mut taps = vec![];
                for option in values {
                    let chosen = if multi {
                        value.as_array().is_some_and(|a| a.contains(option))
                    } else {
                        value == option
                    };
                    let mark = if multi {
                        if chosen { "✓ " } else { "" }
                    } else if chosen {
                        "● "
                    } else {
                        "○ "
                    };
                    taps.push(c.tap(
                        format!("{mark}{}", short(&python(option), 24)),
                        state.clone(),
                        if multi {
                            Operation::Toggle {
                                value: option.clone(),
                            }
                        } else {
                            Operation::Set {
                                value: option.clone(),
                            }
                        },
                        if chosen {
                            Style::Success
                        } else {
                            Style::Default
                        },
                    ));
                }
                let width = if taps.len() > 4 || taps.len() == 3 {
                    3
                } else {
                    2
                };
                for chunk in taps.chunks(width) {
                    c.add(row(chunk));
                }
            }
            Some(Validator::Integer { .. }) | Some(Validator::Float { .. }) => {
                let integer = k == "Integer";
                let current = value.as_f64().or_else(|| default.as_f64()).unwrap_or(0.);
                let steps: [f64; 4] = if integer {
                    [-10., -1., 1., 10.]
                } else {
                    [-1., -0.1, 0.1, 1.]
                };
                let mut taps = vec![];
                for step in steps {
                    let label = format!("{}{v}", if step > 0. { "+" } else { "−" }, v = step.abs());
                    let target = if integer {
                        json!((current + step) as i64)
                    } else {
                        json!(((current + step) * 1e6).round() / 1e6)
                    };
                    let valid = v.unwrap().validate(target).is_ok();
                    taps.push(if valid {
                        c.tap(
                            label,
                            state.clone(),
                            Operation::Step { step, integer },
                            Style::Default,
                        )
                    } else {
                        badge(label, "")
                    });
                }
                taps.insert(2, badge(current.to_string(), "primary"));
                c.add(row(&taps));
            }
            Some(Validator::Series { .. }) => {
                if let Some(items) = value.as_array() {
                    for (index, item) in items.iter().enumerate().take(30) {
                        let tap = c.tap(
                            "✕",
                            state.clone(),
                            Operation::RemoveItem {
                                index,
                                item: item.clone(),
                            },
                            Style::Default,
                        );
                        c.add(format!("{tap} <code>{}</code>", escape(&python(item))));
                    }
                    if items.is_empty() {
                        c.add(format!("<i>{}</i>", ui.text("empty")));
                    }
                    if items.len() > 30 {
                        c.add(format!(
                            "<i>{}</i>",
                            ui.f("more_items", &[(items.len() - 30).to_string()])
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    if k == "Series" {
        let mut keys = vec![key(
            format!("➕ {}", ui.text("add_btn")),
            action(state.clone(), Operation::Input { mode: Input::Add }),
        )];
        if value.as_array().is_some_and(|a| a.len() > 30) {
            keys.push(key(
                format!("✕ {}", ui.text("remove_btn")),
                action(
                    state.clone(),
                    Operation::Input {
                        mode: Input::Remove,
                    },
                ),
            ));
        }
        c.keyboard(keys);
    }
    let mut keys = vec![];
    if k != "Boolean" {
        keys.push(key(
            format!("✎ {}", ui.text("edit_btn")),
            action(state.clone(), Operation::Input { mode: Input::Value }),
        ));
    }
    let copy = if let Some(a) = value.as_array() {
        a.iter().map(python).collect::<Vec<_>>().join(", ")
    } else if value.is_null() {
        String::new()
    } else {
        python(value)
    };
    if !hidden
        && !matches!(k, "Boolean" | "Choice")
        && !copy.is_empty()
        && copy.chars().count() <= 256
    {
        keys.push(key(
            format!("📋 {}", ui.text("copy_btn")),
            Action::Copy { text: copy },
        ));
    }
    if m.changed(p) {
        keys.push(key(
            format!("↺ {}", ui.text("default_btn")),
            action(state.clone(), Operation::Reset),
        ));
    }
    c.keyboard(keys);
    c.footer(Some(back(ui, module_state(state, &m.name))));
    Ok(c.finish((!text_kind).then(|| state.clone())))
}
fn bounds(min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(a), Some(b)) => format!("{a}…{b}"),
        (Some(a), _) => format!("≥ {a}"),
        (_, Some(b)) => format!("≤ {b}"),
        _ => String::new(),
    }
}
fn validator_doc(ui: Ui, validator: &Validator) -> String {
    let t = |key| crate::original::text(ui.english, "$validators", key);
    match validator {
        Validator::String { min_len, max_len } => match (min_len, max_len) {
            (Some(a), Some(b)) => t("string_len_range")
                .replace("{min_len}", &a.to_string())
                .replace("{max_len}", &b.to_string()),
            (Some(a), _) => t("string_min_len").replace("{min_len}", &a.to_string()),
            (_, Some(b)) => t("string_max_len").replace("{max_len}", &b.to_string()),
            _ => t("string"),
        },
        Validator::Series {
            item,
            min_len,
            max_len,
        } => {
            let len = match (min_len, max_len) {
                (Some(a), Some(b)) => t("len_range")
                    .replace("{min_len}", &a.to_string())
                    .replace("{max_len}", &b.to_string()),
                (Some(a), _) => t("min_len").replace("{min_len}", &a.to_string()),
                (_, Some(b)) => t("max_len").replace("{max_len}", &b.to_string()),
                _ => String::new(),
            };
            t("series").replace("{len}", &len).replace(
                "{each}",
                &t("each").replace("{each}", &validator_doc(ui, item)),
            )
        }
        Validator::Hidden { inner } => validator_doc(ui, inner),
        Validator::Link => t("link"),
        Validator::TelegramId => "Telegram ID".into(),
        Validator::None => t("empty"),
        Validator::Union { items } => format!(
            "{}{}",
            t("union"),
            items
                .iter()
                .map(|i| format!("- {}\n", validator_doc(ui, i)))
                .collect::<String>()
        ),
        Validator::Boolean => t("boolean"),
        Validator::Integer { .. } => t("integer").replace("{sign}", "").replace("{digits}", ""),
        Validator::Float { .. } => t("float").replace("{sign}", ""),
        Validator::Choice { values } | Validator::MultiChoice { values } => {
            t(if matches!(validator, Validator::Choice { .. }) {
                "choice"
            } else {
                "multichoice"
            })
            .replace(
                "{possible}",
                &values.iter().map(python).collect::<Vec<_>>().join(" / "),
            )
        }
    }
}
pub fn warn(screen: &mut Screen, text: &str) {
    let warning = format!("⚠ {}", escape(text));
    screen.text = format!("{warning}\n{}", screen.text);
    if let Some(rich) = &mut screen.rich_html {
        *rich = format!("{warning}<br>{rich}");
    }
}

/// Safe, non-executable subset of Python literals used by ModuleConfig (never eval).
pub fn literal(text: &str) -> Value {
    literal_inner(text, 0)
}
fn literal_inner(text: &str, depth: usize) -> Value {
    if depth >= 32 {
        return Value::String(text.into());
    }
    if let Ok(value) = serde_json::from_str(text) {
        return value;
    }
    match text.trim() {
        "True" => return json!(true),
        "False" => return json!(false),
        "None" => return Value::Null,
        _ => {}
    }
    let s = text.trim();
    if ((s.starts_with('[') && s.ends_with(']')) || (s.starts_with('(') && s.ends_with(')')))
        && let Some(items) = split_literals(&s[1..s.len() - 1])
    {
        return Value::Array(
            items
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .map(|s| literal_inner(s.trim(), depth + 1))
                .collect(),
        );
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return json!(
            s[1..s.len() - 1]
                .replace("\\'", "'")
                .replace("\\n", "\n")
                .replace("\\\\", "\\")
        );
    }
    Value::String(text.into())
}
pub fn arguments(text: &str) -> Vec<String> {
    // shlex-like quoting for module/parameter names; broken quotes use whitespace splitting.
    let (mut parts, mut current, mut quote, mut escaped) = (vec![], String::new(), None, false);
    for c in text.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                current.push(c);
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if quote.is_some() || escaped {
        return text.split_whitespace().map(str::to_owned).collect();
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}
fn split_literals(s: &str) -> Option<Vec<&str>> {
    let mut parts = vec![];
    let (mut start, mut depth, mut quote, mut escaped) = (0, 0i32, None, false);
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '[' | '(' => depth += 1,
            ']' | ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return None;
    }
    parts.push(&s[start..]);
    Some(parts)
}
pub fn mutate(
    module: &Module,
    state: &State,
    operation: &Operation,
    input: Option<&str>,
) -> Result<(String, Value)> {
    let p = state.parameter.as_deref().context("parameter missing")?;
    let current = module.values.get(p).context("parameter removed")?;
    let value = match operation {
        Operation::Set { value } => value.clone(),
        Operation::Reset => module.defaults[p].clone(),
        Operation::Toggle { value } => {
            let mut a = current.as_array().cloned().unwrap_or_default();
            if a.contains(value) {
                a.retain(|v| v != value);
            } else {
                a.push(value.clone());
            }
            json!(a)
        }
        Operation::Step { step, integer } => {
            let n = current
                .as_f64()
                .or_else(|| module.defaults[p].as_f64())
                .unwrap_or(0.)
                + step;
            if *integer {
                ensure!(
                    n >= i64::MIN as f64 && n < i64::MAX as f64,
                    "integer overflow"
                );
                json!(n as i64)
            } else {
                json!((n * 1e6).round() / 1e6)
            }
        }
        Operation::RemoveItem { index, item } => {
            let mut a = current.as_array().cloned().unwrap_or_default();
            let i = if a.get(*index) == Some(item) {
                *index
            } else {
                a.iter().position(|v| v == item).context("not_in_list")?
            };
            a.remove(i);
            json!(a)
        }
        Operation::Input { mode } => {
            let v = literal(input.context("input missing")?);
            match mode {
                Input::Value => v,
                Input::Add | Input::Remove => {
                    let items = if let Value::Array(a) = v { a } else { vec![v] };
                    let mut a = current.as_array().cloned().unwrap_or_default();
                    if *mode == Input::Add {
                        a.extend(items);
                    } else {
                        let before = a.len();
                        a.retain(|v| !items.iter().any(|i| python(i) == python(v)));
                        ensure!(a.len() != before, "not_in_list");
                    }
                    json!(a)
                }
            }
        }
        _ => anyhow::bail!("not a parameter change"),
    };
    let value = if let Some(v) = module.validators.get(p) {
        v.validate(config_value(value, Some(v)))?
    } else {
        value
    };
    Ok((p.into(), value))
}
pub fn config_value(value: Value, validator: Option<&Validator>) -> Value {
    if value.is_null() {
        match validator {
            Some(Validator::String { .. }) => json!(""),
            Some(Validator::Integer { .. }) => json!(0),
            Some(Validator::Boolean) => json!(false),
            Some(Validator::Float { .. }) => json!(0.),
            Some(Validator::Series { .. }) => json!([]),
            _ => Value::Null,
        }
    } else if let Value::Array(a) = value {
        Value::Array(
            a.into_iter()
                .map(|v| {
                    if let Value::String(s) = v {
                        json!(s.trim())
                    } else {
                        v
                    }
                })
                .collect(),
        )
    } else {
        value
    }
}

pub fn ui(ctx: &crate::commands::Context) -> Ui {
    Ui {
        english: ctx.db.language().starts_with("en"),
        premium: ctx
            .db
            .get("RustRuntime", "premium")
            .as_bool()
            .unwrap_or(false),
    }
}
pub fn modules(ctx: &crate::commands::Context) -> Result<Vec<Module>> {
    let mut modules = vec![];
    for name in [
        crate::info::MODULE,
        crate::update_ui::MODULE,
        "Settings",
        "Tester",
    ] {
        let (values, defaults, validators) = crate::screens::config_data(ctx, name)?;
        let mut docs = BTreeMap::new();
        for key in defaults.as_object().into_iter().flat_map(|m| m.keys()) {
            let source = match (name, key.as_str()) {
                ("Updater", "update_notifications") => Some(("updater", "_cfg_notify")),
                ("Settings", "allow_nonstandart_prefixes") => {
                    Some(("settings", "_cfg_nonstandart_prefixes"))
                }
                ("ataraxiaInfo", "custom_message") => Some(("ataraxia_info", "_cfg_cst_msg")),
                ("ataraxiaInfo", "rich_mode") => Some(("ataraxia_info", "_cfg_rich_mode")),
                ("ataraxiaInfo", "banner_url") => Some(("ataraxia_info", "_cfg_banner")),
                ("Tester", "custom_message") => Some(("test", "configping")),
                ("Tester", "rich_mode") => Some(("test", "rich_mode")),
                ("Tester", "hint") => Some(("test", "hint")),
                ("Tester", "ping_emoji") => Some(("test", "ping_emoji")),
                ("Tester", "banner_url") => Some(("test", "banner_url")),
                _ => None,
            };
            if let Some((m, k)) = source {
                docs.insert(key.clone(), crate::original::text(ui(ctx).english, m, k));
            }
        }
        modules.push(Module {
            name: name.into(),
            kind: "core",
            values,
            defaults,
            validators,
            docs,
            folders: BTreeMap::new(),
        });
    }
    for package in ctx.modules.lock().unwrap().export() {
        let m = package.manifest;
        if !m.config.as_object().is_some_and(|o| !o.is_empty()) {
            continue;
        }
        let mut values = m.config.clone();
        if let Some(saved) = ctx.db.get(&m.name, "__config__").as_object() {
            for (k, v) in saved {
                values[k] = v.clone();
            }
        }
        modules.push(Module {
            name: m.name,
            kind: "external",
            values,
            defaults: m.config,
            validators: m.validators,
            docs: m.config_docs,
            folders: m.config_folders,
        });
    }
    Ok(modules)
}
pub fn build(ctx: &crate::commands::Context, state: &State, unit: Option<&str>) -> Result<Screen> {
    let modules = modules(ctx)?;
    let mut screen = render(&modules, state, ui(ctx))?;
    if state
        .module
        .as_ref()
        .is_some_and(|name| selected(&modules, name).is_some())
        && state.parameter.is_none()
        && !state.confirm_reset
        && state.folder.is_none()
        && let Some(back) = unit.and_then(|id| ctx.forms.lock().unwrap().help_return(id))
    {
        let row = screen.buttons.last_mut().unwrap();
        row[0] = key(
            format!(
                "‹ {}",
                crate::original::text(ui(ctx).english, "$inline", "screen_back")
            ),
            Action::HelpNavigate { state: back },
        );
    }
    Ok(screen)
}
pub fn apply(
    ctx: &crate::commands::Context,
    state: &State,
    op: &Operation,
    input: Option<&str>,
) -> Result<()> {
    let modules = modules(ctx)?;
    let m = selected(&modules, state.module.as_deref().context("module missing")?)
        .context("module removed")?;
    if matches!(op, Operation::ResetAll) {
        for p in m.keys() {
            crate::screens::set_config(ctx, &m.name, p, m.defaults[p].clone())?;
        }
        return Ok(());
    }
    let (p, value) = mutate(m, state, op, input)?;
    crate::screens::set_config(ctx, &m.name, &p, value)
}
pub fn changed_screen(
    ctx: &crate::commands::Context,
    state: &State,
    op: &Operation,
    input: Option<&str>,
    unit: &str,
) -> Result<(Screen, Option<String>)> {
    let result = apply(ctx, state, op, input);
    let target = if matches!(op, Operation::ResetAll) {
        module_state(state, state.module.as_deref().unwrap_or(""))
    } else {
        State {
            show: false,
            ..state.clone()
        }
    };
    let mut screen = build(ctx, &target, Some(unit))?;
    match result {
        Ok(()) => {
            let toast = ui(ctx).text(if matches!(op, Operation::ResetAll) {
                "reset_done"
            } else {
                "saved"
            });
            if input.is_some() {
                screen.text = format!("{}\n{}", escape(&toast), screen.text);
                if let Some(r) = &mut screen.rich_html {
                    *r = format!("{}<br>{r}", escape(&toast));
                }
                Ok((screen, None))
            } else {
                Ok((screen, Some(toast)))
            }
        }
        Err(e) => {
            warn(
                &mut screen,
                &if e.to_string() == "not_in_list" {
                    ui(ctx).text("not_in_list")
                } else if state
                    .module
                    .as_ref()
                    .and_then(|name| {
                        modules(ctx).ok().and_then(|modules| {
                            selected(&modules, name)
                                .and_then(|m| {
                                    state.parameter.as_ref().and_then(|p| m.validators.get(p))
                                })
                                .map(Validator::hidden)
                        })
                    })
                    .unwrap_or(false)
                {
                    ui(ctx)
                        .text("validation_error")
                        .replace("{}", &ui(ctx).text("type"))
                } else {
                    e.to_string()
                },
            );
            Ok((screen, None))
        }
    }
}

pub fn settings(ctx: &crate::commands::Context) -> Screen {
    settings_screen(&ctx.db, ui(ctx).english)
}
pub fn settings_screen(db: &crate::database::Database, english: bool) -> Screen {
    let t = |k| crate::original::text(english, "settings", k);
    let mut rows = vec![];
    let mut row = vec![];
    for (key, label, default) in [
        ("no_nickname", "NoNick", false),
        ("grep", "Grep", false),
        ("inlinelogs", "InlineLogs", true),
    ] {
        let value = db.get("atarustia.main", key).as_bool().unwrap_or(default);
        row.push(key_button(
            format!("{} {label}", if value { "✅" } else { "🚫" }),
            key,
            !value,
        ));
    }
    rows.push(row);
    let suggest = db
        .get("atarustia.main", "suggest_subscribe")
        .as_bool()
        .unwrap_or(true);
    rows.push(vec![key_button(
        t(if suggest {
            "suggest_subscribe"
        } else {
            "do_not_suggest_subscribe"
        }),
        "suggest_subscribe",
        !suggest,
    )]);
    rows.push(vec![
        key(
            t("btn_restart"),
            Action::SettingsMaintenance {
                restart: true,
                confirmed: false,
            },
        ),
        key(
            t("btn_update"),
            Action::SettingsMaintenance {
                restart: false,
                confirmed: false,
            },
        ),
    ]);
    rows.push(vec![key(t("close_menu"), Action::Close)]);
    let mut styles: Vec<Vec<Appearance>> = rows
        .iter()
        .map(|r| vec![Appearance::default(); r.len()])
        .collect();
    for a in &mut styles[2] {
        a.style = Style::Primary;
    }
    styles[3][0].style = Style::Danger;
    Screen {
        text: t("inline_settings"),
        html: true,
        buttons: rows,
        button_appearance: styles,
        close_button: false,
        ..Default::default()
    }
}
fn key_button(text: String, name: &str, value: bool) -> Key {
    key(
        text,
        Action::Settings {
            key: Some(name.into()),
            value,
        },
    )
}
pub fn settings_confirmation(english: bool, restart: bool) -> Screen {
    Screen {
        text: crate::original::text(
            english,
            "settings",
            if restart {
                "confirm_restart"
            } else {
                "confirm_update"
            },
        ),
        html: true,
        buttons: vec![vec![
            key(
                if restart {
                    "🔄 Restart"
                } else {
                    "🪂 Update"
                },
                Action::SettingsMaintenance {
                    restart,
                    confirmed: true,
                },
            ),
            key("🚫 Cancel", Action::Close),
        ]],
        button_appearance: vec![vec![
            Appearance {
                style: Style::Primary,
                emoji_id: None,
            },
            Appearance {
                style: Style::Danger,
                emoji_id: None,
            },
        ]],
        close_button: false,
        ..Default::default()
    }
}

pub fn fast_config(
    ctx: &crate::commands::Context,
    raw: &str,
    reply: Option<&str>,
) -> Result<String> {
    let u = ui(ctx);
    let parts: Vec<_> = raw
        .split("&&")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    let Some(first) = parts.first() else {
        return Ok(u.text("args"));
    };
    let (name, rest) = first.split_once(char::is_whitespace).unwrap_or((first, ""));
    let rest = rest.trim_start();
    let (p, value) = rest
        .split_once(char::is_whitespace)
        .map(|(p, v)| (p, Some(v.trim_start())))
        .unwrap_or((rest, None));
    if p.is_empty() {
        return Ok(u.text("args"));
    }
    let Some(value) = value.or(reply).filter(|v| !v.is_empty()) else {
        return Ok(u.text("args"));
    };
    let all = modules(ctx)?;
    let Some(m) = selected(&all, name) else {
        return Ok(u.text("no_mod"));
    };
    let mut updates = vec![];
    for (index, part) in parts.iter().enumerate() {
        let (p, value) = if index == 0 {
            (p, value)
        } else {
            let Some((p, v)) = part.split_once(char::is_whitespace) else {
                return Ok(u.text("args"));
            };
            (p, v.trim_start())
        };
        if m.defaults.get(p).is_none() {
            return Ok(u.text("no_option"));
        }
        let state = State {
            module: Some(m.name.clone()),
            parameter: Some(p.into()),
            ..Default::default()
        };
        match apply(
            ctx,
            &state,
            &Operation::Input { mode: Input::Value },
            Some(value),
        ) {
            Err(e) => {
                updates.push(u.f("validation_error", &[escape(&e.to_string())]));
                return Ok(updates.join("\n"));
            }
            Ok(()) => {
                let (current, _, validators) = crate::screens::config_data(ctx, &m.name)?;
                let v = &current[p];
                let shown = if validators.get(p).is_some_and(Validator::hidden) {
                    "*".repeat(python(v).chars().count())
                } else {
                    python(v)
                };
                let shown = format!("</b><code>{}</code><b>", escape(shown.trim()));
                updates.push(u.f("option_saved", &[escape(p), escape(name), shown]));
            }
        }
    }
    Ok(updates.join("\n"))
}

async fn edit_unit(ctx: &crate::commands::Context, unit: &str, screen: Screen) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if ctx.forms.lock().unwrap().inline_id(unit).is_some() {
            break;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "inline result ID has not arrived"
        );
        tokio::select! {_=ctx.stop.cancelled()=>anyhow::bail!("account stopped"),_=tokio::time::sleep(std::time::Duration::from_millis(50))=>{}}
    }
    let (body, rich, id) = {
        let mut forms = ctx.forms.lock().unwrap();
        let body = forms.replace(unit, screen)?;
        (
            body,
            forms.rich_source(unit),
            forms
                .inline_id(unit)
                .context("inline result ID has not arrived")?,
        )
    };
    let bot = ctx
        .bot
        .read()
        .await
        .clone()
        .context("inline bot unavailable")?;
    if let Err(error) = bot
        .bot
        .edit_inline_message(&id, body, None, rich.as_deref())
        .await
    {
        if rich.is_none() {
            return Err(error.into());
        }
        let body = {
            let mut forms = ctx.forms.lock().unwrap();
            forms.plain_fallback(unit);
            forms.render(unit, 0)?
        };
        bot.bot.edit_inline_message(&id, body, None, None).await?;
    }
    Ok(())
}
pub async fn handle_reply(
    ctx: &crate::commands::Context,
    message: &grammers_client::message::Message,
) -> Result<bool> {
    let Some(user) = message.sender_id().map(|s| s.bare_id_unchecked()) else {
        return Ok(false);
    };
    if !crate::security::owners(&ctx.db, ctx.owner).contains(&user) {
        return Ok(false);
    }
    let Some(reply) = message.reply_to_message_id() else {
        return Ok(false);
    };
    // Commands replying to a screen still go through the dispatcher, not into its value.
    if matches!(
        crate::dispatcher::parse(
            message.text(),
            &ctx.db.prefix_for(Some(user), ctx.owner),
            ctx.username.as_deref(),
            message.outgoing()
        ),
        crate::dispatcher::Parsed::Command { .. }
    ) {
        return Ok(false);
    }
    let target = ctx
        .forms
        .lock()
        .unwrap()
        .config_reply(message.peer_id().bot_api_dialog_id_unchecked(), reply);
    let Some((unit, state)) = target else {
        return Ok(false);
    };
    let (screen, _) = changed_screen(
        ctx,
        &state,
        &Operation::Input { mode: Input::Value },
        Some(message.text()),
        &unit,
    )?;
    crate::module_lifecycle::config_changes(ctx).await;
    edit_unit(ctx, &unit, screen).await?;
    message.delete().await?;
    Ok(true)
}
pub async fn handle_bot_update(
    ctx: &crate::commands::Context,
    update: &grammers_client::update::Update,
) -> Result<bool> {
    use grammers_client::{
        tl,
        update::{Article, Update},
    };
    if let tl::enums::Update::BotInlineSend(sent) = update.raw() {
        let owners = crate::security::owners(&ctx.db, ctx.owner);
        if !owners.contains(&sent.user_id) {
            return Ok(false);
        }
        if let Some(unit) = sent.query.strip_prefix("unit:") {
            if let Some(id) = &sent.msg_id {
                ctx.forms
                    .lock()
                    .unwrap()
                    .bind_inline(unit, crate::file_id::encode_inline_id(id));
            }
            return Ok(true);
        }
        let target = ctx.forms.lock().unwrap().config_input(&sent.query);
        if let Some((unit, state, mode, value)) = target {
            let (screen, _) =
                changed_screen(ctx, &state, &Operation::Input { mode }, Some(&value), &unit)?;
            crate::module_lifecycle::config_changes(ctx).await;
            edit_unit(ctx, &unit, screen).await?;
            return Ok(true);
        }
    }
    if let Update::InlineQuery(query) = update {
        let target = ctx.forms.lock().unwrap().config_input(query.text());
        if let Some((_, state, mode, _)) = target {
            if !crate::security::owners(&ctx.db, ctx.owner)
                .contains(&query.sender_id().bare_id_unchecked())
            {
                query
                    .answer(Vec::<Article>::new())
                    .private()
                    .cache_time(0)
                    .send()
                    .await?;
                return Ok(true);
            }
            let u = ui(ctx);
            let title = match mode {
                Input::Value => u.f("edit_prompt", &[state.parameter.unwrap_or_default()]),
                Input::Add => u.text("add_prompt"),
                Input::Remove => u.text("remove_prompt"),
            };
            let body = if query.sender_id().bare_id_unchecked() == ctx.owner {
                "🔄 <b>Transferring value to userbot...</b>\n<i>This message will be deleted automatically</i>"
            } else {
                "🔄 <b>Transferring value to userbot...</b>"
            };
            query
                .answer([Article::new(
                    title,
                    grammers_client::message::InputMessage::new()
                        .html(body)
                        .link_preview(false),
                )])
                .private()
                .cache_time(60)
                .send()
                .await?;
            return Ok(true);
        }
    }
    if let Update::NewMessage(message) = update
        && message.outgoing()
        && message.via_bot_id().is_some()
        && message
            .text()
            .contains("This message will be deleted automatically")
    {
        let own_bot = ctx.bot.read().await.as_ref().map(|b| b.bot.id);
        if message.via_bot_id() == own_bot {
            message.delete().await?;
            return Ok(true);
        }
    }
    Ok(false)
}
