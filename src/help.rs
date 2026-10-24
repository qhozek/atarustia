//! Permission-aware module catalogue and rich help navigation with a real plain fallback.
use crate::{
    commands::Context,
    forms::{Action, Key, Screen},
    output::escape,
    security::{self, Permission},
};
use anyhow::{Context as _, Result, ensure};
use grammers_client::{message::Message, update::CallbackQuery};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
pub const PAGE_SIZE: usize = 15;
pub const COMMAND_PAGE_SIZE: usize = 8;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Metadata {
    pub version: Option<String>,
    pub developer: Option<String>,
    pub banner_url: Option<String>,
}
impl Metadata {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version.as_ref().is_none_or(|s| s.len() <= 64)
                && self.developer.as_ref().is_none_or(|s| s.len() <= 256),
            "help metadata too long"
        );
        if let Some(url) = &self.banner_url {
            let u = reqwest::Url::parse(url)?;
            ensure!(
                url.len() <= 8192
                    && u.scheme() == "https"
                    && u.host_str().is_some()
                    && u.username().is_empty()
                    && u.password().is_none(),
                "help banner must be HTTPS without credentials"
            );
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Filter {
    #[default]
    All,
    Core,
    External,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub filter: Filter,
    pub force: bool,
    pub page: usize,
    pub hidden: bool,
    pub module: Option<String>,
    pub command_page: usize,
    pub closest: bool,
}
impl State {
    pub fn parse(args: &str) -> Result<Self> {
        ensure!(args.len() <= 1024, "help query too long");
        let mut state = Self::default();
        let mut query = vec![];
        for part in args.split_whitespace() {
            match part {
                "-f" => state.force = true,
                "-c" => {
                    state.filter = Filter::Core;
                    state.force = true;
                }
                "-l" => {
                    state.filter = Filter::External;
                    state.force = true;
                }
                _ => query.push(part),
            }
        }
        let query = query.join(" ");
        if !query.is_empty() {
            state.module = Some(query);
        }
        Ok(state)
    }
    fn home(&self) -> Self {
        Self {
            module: None,
            command_page: 0,
            closest: false,
            ..self.clone()
        }
    }
}
/// Usage prefixes are displayed separately, as in the original command cards.
pub fn parse_doc(doc: &str) -> (String, String) {
    let mut rest = doc.trim();
    let mut usage = vec![];
    loop {
        let close = match rest.chars().next() {
            Some('[') => ']',
            Some('<') => '>',
            _ => break,
        };
        let Some(end) = rest.find(close) else {
            break;
        };
        usage.push(rest[..=end].to_owned());
        rest = rest[end + 1..].trim_start();
    }
    (
        usage.join(" "),
        rest.trim_start_matches(['-', '|', '—', ':', ' '])
            .to_owned(),
    )
}
#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub permission: Permission,
    pub inline: bool,
    pub guest: bool,
    pub text_allowed: bool,
}
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub description: String,
    pub core: bool,
    pub configurable: bool,
    pub hidden: bool,
    pub commands: Vec<Command>,
    pub metadata: Metadata,
}
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub modules: Vec<Module>,
    pub libraries: Vec<String>,
}
fn command(name: &str, description: &str) -> Command {
    Command {
        name: name.into(),
        description: description.into(),
        permission: Permission::Owner,
        inline: false,
        guest: false,
        text_allowed: true,
    }
}
pub fn core_group(name: &str) -> &'static str {
    match name {
        "info" | "help" | "helphide" => crate::info::MODULE,
        "update" | "restart" | "source" => crate::update_ui::MODULE,
        "config" | "fconfig" => "ataraxiaConfig",
        "weburl" | "addacc" => "ataraxiaWeb",
        "backupall" | "backupdb" | "backupmods" | "restoreall" | "restoredb" | "restoremods"
        | "set_backup_period" => "ataraxiaBackup",
        "e" | "terminal" | "terminate" | "ec" | "ecpp" | "enode" => "Executor",
        "dlm" | "lm" | "ulm" | "ml" | "clearmodules" => "Loader",
        "api_fw_protection" | "suspend_api_protect" => "APILimiter",
        "setlang" | "dllangpack" => "Translations",
        "ping" | "logs" | "clearlogs" | "suspend" => "Tester",
        "ch_ataraxia_bot" | "ch_bot_token" => "InlineStuff",
        name if crate::security_commands::COMMANDS
            .iter()
            .any(|(n, _)| *n == name)
            || matches!(
                name,
                "security" | "owneradd" | "ownerrm" | "ownerlist" | "tsec"
            ) =>
        {
            "ataraxiaSecurity"
        }
        _ => "Settings",
    }
}
pub fn catalog(ctx: &Context) -> Catalog {
    let mut core: BTreeMap<&str, Module> = BTreeMap::new();
    for (name, _) in crate::commands::BUILTINS
        .iter()
        .chain(crate::preferences::COMMANDS)
        .chain(crate::security_commands::COMMANDS)
    {
        let group = core_group(name);
        let source = crate::original::module_key(group);
        let english = ctx.db.language().starts_with("en");
        let m = core.entry(group).or_insert_with(|| Module {
            name: group.into(),
            description: crate::original::module_description(english, group),
            core: true,
            configurable: matches!(
                group,
                crate::info::MODULE | crate::update_ui::MODULE | "Settings" | "Tester"
            ),
            hidden: crate::preferences::hidden(&ctx.db, group),
            commands: vec![],
            metadata: Metadata::default(),
        });
        let key = format!("_cmd_doc_{name}");
        let mut description = crate::original::text(english, source, &key);
        if description == key {
            description = match *name {
                "ml" => "| send module via file".into(),
                "clearmodule" => "Clear all DB entries for module: clearmodule <module>".into(),
                _ => String::new(),
            };
        }
        m.commands.push(command(name, &description));
    }
    let mut catalog = Catalog {
        modules: core.into_values().collect(),
        libraries: vec![],
    };
    catalog.modules.push(Module {
        name: "Quickstart".into(),
        description: crate::original::text(
            ctx.db.language().starts_with("en"),
            "quickstart",
            "_cls_doc",
        ),
        core: true,
        configurable: false,
        hidden: crate::preferences::hidden(&ctx.db, "Quickstart"),
        commands: vec![],
        metadata: Metadata::default(),
    });
    for module in &mut catalog.modules {
        module.commands.sort_by(|a, b| a.name.cmp(&b.name));
    }
    for package in ctx.modules.lock().unwrap().export() {
        let manifest = package.manifest;
        catalog.libraries.extend(manifest.libraries.keys().cloned());
        catalog.modules.push(Module {
            name: manifest.name.clone(),
            description: manifest.description,
            core: false,
            configurable: manifest.config.as_object().is_some_and(|m| !m.is_empty()),
            hidden: crate::preferences::hidden(&ctx.db, &manifest.name),
            commands: manifest
                .commands
                .into_iter()
                .map(|c| Command {
                    name: c.name,
                    description: c.description,
                    permission: c.permission,
                    inline: c.inline,
                    guest: c.guest,
                    text_allowed: true,
                })
                .collect(),
            metadata: manifest.help.unwrap_or_default(),
        });
    }
    for module in ctx.native.list() {
        catalog.modules.push(Module {
            name: module.name().into(),
            description: module.description().into(),
            core: false,
            configurable: false,
            hidden: crate::preferences::hidden(&ctx.db, module.name()),
            commands: module
                .commands()
                .into_iter()
                .map(|c| Command {
                    name: c.name,
                    description: c.description,
                    permission: c.permission,
                    inline: false,
                    guest: false,
                    text_allowed: true,
                })
                .collect(),
            metadata: module.help_metadata(),
        });
    }
    catalog
        .modules
        .sort_by_key(|m| (m.commands.is_empty(), m.name.to_lowercase()));
    catalog
}

/// Permissions are evaluated for the person viewing the card, never for an unrelated form owner.
pub async fn visible(
    ctx: &Context,
    user: i64,
    origin: Option<&Message>,
    catalog: &Catalog,
) -> Result<(Catalog, bool)> {
    let mut result = catalog.clone();
    let mut restricted = false;
    for module in &mut result.modules {
        let mut commands = vec![];
        for cmd in &module.commands {
            let permission = ctx.permission_with_default(&cmd.name, cmd.permission);
            let allowed = if let Some(message) = origin.filter(|m| {
                m.sender_id()
                    .is_some_and(|id| id.bare_id_unchecked() == user)
            }) {
                ctx.access
                    .check(
                        &ctx.client,
                        &ctx.db,
                        ctx.owner,
                        message,
                        &cmd.name,
                        if module.core {
                            None
                        } else {
                            Some(&module.name)
                        },
                        permission,
                    )
                    .await?
            } else {
                security::allowed(&ctx.db, ctx.owner, Some(user), user, &cmd.name, permission)
                    || !security::owner_only(&cmd.name)
                        && security::targeted(
                            &ctx.db,
                            user,
                            user,
                            &cmd.name,
                            if module.core {
                                None
                            } else {
                                Some(&module.name)
                            },
                            false,
                        )
            };
            let inline = cmd.inline
                && crate::security_commands::inline_allowed(&ctx.db, ctx.owner, user, &cmd.name);
            let guest = cmd.guest && security::owners(&ctx.db, ctx.owner).contains(&user);
            if !allowed {
                restricted = true;
            }
            if allowed || inline || guest {
                let mut c = cmd.clone();
                c.inline = inline;
                c.guest = guest;
                commands.push((c, allowed));
            }
        }
        // The separate visibility map preserves inline-only handlers without pretending their text command is allowed.
        module.commands = commands
            .into_iter()
            .map(|(mut cmd, allowed)| {
                cmd.text_allowed = allowed;
                cmd
            })
            .collect();
    }
    result.modules.retain(|m| {
        !m.commands.is_empty()
            || catalog
                .modules
                .iter()
                .find(|orig| orig.name == m.name)
                .is_some_and(|orig| orig.commands.is_empty())
    });
    Ok((result, restricted))
}
#[derive(Clone, Copy)]
pub struct Ui {
    pub english: bool,
    pub premium: bool,
}
impl Ui {
    pub fn text(&self, key: &str, values: &[(&str, String)]) -> String {
        let source_key = match key {
            "prefix" => "prefix_badge",
            "settings" => "settings_btn",
            "hide" => "hide_btn",
            "unhide" => "unhide_btn",
            "back" => "screen_back",
            "close" => "screen_close",
            "library" => "help_lib",
            _ => key,
        };
        let module = if matches!(key, "back" | "close") {
            "$inline"
        } else {
            "ataraxia_info"
        };
        let positional = values.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>();
        crate::original::message(self.english, module, source_key, &positional, &[])
    }
    fn modules(&self, count: usize) -> String {
        crate::update_ui::Ui {
            english: self.english,
        }
        .plural(count as u64, "modules")
    }
    fn logo(&self) -> String {
        if self.premium {
            crate::update_ui::PREMIUM_LOGO.into()
        } else {
            "<b>ataraxia</b>".into()
        }
    }
}
fn short(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut result = chars.by_ref().take(max).collect::<String>();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}
fn icon(name: &str, premium: bool) -> String {
    let (id, alt, fallback): (i64, &str, &str) = match name {
        "core" => (6023761060786346622, "⚡️", "⛈"),
        "module" => (6024065724291488135, "🧩", "🧩"),
        "hide" => (6021869252246510287, "👁", "🙈"),
        "eye" => (6024008227564296298, "👁", "👁"),
        "command" => (6021607757457660145, "🖥", "⌨"),
        "bot" => (6019462434178211398, "📱", "🤖"),
        "guest" => (6021690418398239007, "👥", "🫂"),
        _ => (6019094432790354513, "💬", "ℹ️"),
    };
    if premium {
        format!("<tg-emoji emoji-id=\"{id}\">{alt}</tg-emoji>")
    } else {
        fallback.into()
    }
}
struct Canvas {
    rich: Vec<String>,
    plain: Vec<String>,
    keys: Vec<Key>,
}
impl Canvas {
    fn new() -> Self {
        Self {
            rich: vec![],
            plain: vec![],
            keys: vec![],
        }
    }
    fn add(&mut self, rich: String, plain: String) {
        self.rich.push(rich);
        self.plain.push(plain);
    }
    fn tap(&mut self, label: String, action: Action) -> String {
        let i = self.keys.len();
        self.keys.push(Key {
            text: label,
            action: Some(action),
            url: None,
        });
        crate::rich::marker(i)
    }
    fn finish(self, buttons: Vec<Vec<Key>>) -> Screen {
        let plain = self.plain.join("\n");
        let mut pages = vec![];
        let mut page = String::new();
        let mut units = 0;
        for line in &self.plain {
            let count = grammers_client::update::Article::new(
                "help",
                grammers_client::message::InputMessage::new().html(line),
            )
            .raw
            .send_message;
            let grammers_client::tl::enums::InputBotInlineMessage::Text(content) = count else {
                unreachable!()
            };
            let count = content.message.encode_utf16().count() + 1;
            if units + count > crate::output::MESSAGE_LIMIT && !page.is_empty() {
                pages.push(std::mem::take(&mut page));
                units = 0;
            }
            if !page.is_empty() {
                page.push('\n');
            }
            page.push_str(line);
            units += count;
        }
        if !page.is_empty() {
            pages.push(page);
        }
        Screen {
            text: plain.clone(),
            pages,
            html: true,
            rich_html: Some(self.rich.join("<br>")),
            rich_buttons: self.keys,
            buttons,
            link_preview: false,
            close_button: false,
            ..Default::default()
        }
    }
}
fn nav(label: impl Into<String>, state: State) -> Key {
    Key {
        text: label.into(),
        action: Some(Action::HelpNavigate { state }),
        url: None,
    }
}
fn close(ui: Ui) -> Key {
    Key {
        text: format!("✕ {}", ui.text("close", &[])),
        action: Some(Action::Close),
        url: None,
    }
}
fn badge(text: &str, style: &str) -> String {
    format!(
        "<tg-button type=\"disabled\"{style}>{}</tg-button>",
        escape(text)
    )
}
fn details(title: &str, body: &str, open: bool) -> String {
    format!(
        "<details{}><summary>{title}</summary>{body}</details>",
        if open { " open" } else { "" }
    )
}
fn pager(state: &State, page: usize, pages: usize, card: bool) -> Vec<Key> {
    if pages <= 1 {
        return vec![];
    }
    let mut keys = vec![];
    let state_at = |p| {
        if card {
            State {
                command_page: p,
                ..state.clone()
            }
        } else {
            State {
                page: p,
                ..state.clone()
            }
        }
    };
    if page > 0 {
        keys.push(nav("‹", state_at(page - 1)));
    }
    keys.push(nav(format!("{} / {}", page + 1, pages), state_at(page)));
    if page + 1 < pages {
        keys.push(nav("›", state_at(page + 1)));
    }
    keys
}
fn entry(canvas: &mut Canvas, module: &Module, state: &State, ui: Ui) -> (String, String) {
    let target = State {
        module: Some(module.name.clone()),
        command_page: 0,
        closest: false,
        ..state.clone()
    };
    let tap = canvas.tap(module.name.clone(), Action::HelpNavigate { state: target });
    let count = module
        .commands
        .iter()
        .map(|c| usize::from(c.text_allowed) + usize::from(c.inline))
        .sum::<usize>();
    let count = if count == 0 {
        ui.text("no_commands_badge", &[])
    } else {
        count.to_string()
    };
    let summary = escape(&short(module.description.lines().next().unwrap_or(""), 48));
    let icon = icon(if module.core { "core" } else { "module" }, ui.premium);
    (
        format!(
            "{icon} {tap} {} <i>{summary}</i>",
            badge(&count, " style=\"primary\"")
        ),
        format!(
            "{icon} <b>{}</b> · {count} <i>{summary}</i>",
            escape(&module.name)
        ),
    )
}
#[allow(clippy::too_many_arguments)] // Pure renderer; identity, permission result and presentation are explicit.
pub fn render(
    catalog: &Catalog,
    state: &State,
    ui: Ui,
    prefix: &str,
    bot: &str,
    owner: bool,
    restricted: bool,
    aliases: &HashMap<String, String>,
    banner: &str,
) -> Result<Screen> {
    let mut screen = render_inner(
        catalog, state, ui, prefix, bot, owner, restricted, aliases, banner,
    )?;
    screen.help_state = Some(state.clone());
    Ok(screen)
}
#[allow(clippy::too_many_arguments)]
fn render_inner(
    catalog: &Catalog,
    state: &State,
    ui: Ui,
    prefix: &str,
    bot: &str,
    owner: bool,
    restricted: bool,
    aliases: &HashMap<String, String>,
    banner: &str,
) -> Result<Screen> {
    ensure!(
        prefix.chars().count() <= 8 && bot.len() <= 64,
        "invalid help display context"
    );
    if let Some(query) = &state.module {
        return card(catalog, state, query, ui, prefix, bot, owner, aliases);
    }
    if state.hidden {
        return hidden(catalog, state, ui, prefix, owner);
    }
    let entries = catalog
        .modules
        .iter()
        .filter(|m| {
            (state.force || !m.hidden)
                && match state.filter {
                    Filter::All => true,
                    Filter::Core => m.core,
                    Filter::External => !m.core,
                }
        })
        .collect::<Vec<_>>();
    let core = entries
        .iter()
        .copied()
        .filter(|m| m.core)
        .collect::<Vec<_>>();
    let external = entries
        .iter()
        .copied()
        .filter(|m| !m.core)
        .collect::<Vec<_>>();
    let pages = external.len().div_ceil(PAGE_SIZE).max(1);
    let page = state.page.min(pages - 1);
    let mut state = state.clone();
    state.page = page;
    let external = external
        .into_iter()
        .skip(page * PAGE_SIZE)
        .take(PAGE_SIZE)
        .collect::<Vec<_>>();
    let mut c = Canvas::new();
    if !banner.is_empty() {
        c.rich.push(format!("<img src=\"{}\"/>", escape(banner)));
    }
    let mut head = format!(
        "{} {} {}",
        ui.logo(),
        badge(&ui.modules(catalog.modules.len()), " style=\"primary\""),
        badge(&ui.text("prefix", &[("prefix", prefix.into())]), "")
    );
    let hidden = catalog.modules.iter().filter(|m| m.hidden).count();
    let mut plain = format!(
        "<b>ataraxia</b> · {} · {}",
        ui.modules(catalog.modules.len()),
        escape(&ui.text("prefix", &[("prefix", prefix.into())]))
    );
    if hidden > 0 && !state.force {
        let label = ui.text("hidden_badge", &[("count", hidden.to_string())]);
        head += &format!(
            " {}",
            c.tap(
                label.clone(),
                Action::HelpNavigate {
                    state: State {
                        hidden: true,
                        ..state.home()
                    }
                }
            )
        );
        plain += &format!(" · {}", escape(&label));
    }
    c.add(head, plain);
    if restricted {
        let line = format!("<i>{}</i>", ui.text("restricted", &[]));
        c.add(line.clone(), line);
    }
    for (modules, title, open) in [
        (&core, "builtin_section", external.is_empty()),
        (&external, "external_section", true),
    ] {
        if modules.is_empty() {
            continue;
        }
        let mut rich = vec![];
        let mut plain = vec![];
        for module in modules {
            let (r, p) = entry(&mut c, module, &state, ui);
            rich.push(r);
            plain.push(p);
        }
        let title = format!("<b>{}</b> · {}", ui.text(title, &[]), modules.len());
        c.add(
            details(&title, &rich.join("<br>"), open),
            format!("{title}\n{}", plain.join("\n")),
        );
    }
    if entries.is_empty() {
        c.add(ui.text("nothing", &[]), ui.text("nothing", &[]));
    }
    let nav = pager(&state, page, pages, false);
    let mut rows = vec![];
    if !nav.is_empty() {
        rows.push(nav);
    }
    rows.push(vec![close(ui)]);
    Ok(c.finish(rows))
}

fn hidden(catalog: &Catalog, state: &State, ui: Ui, prefix: &str, owner: bool) -> Result<Screen> {
    let all = catalog
        .modules
        .iter()
        .filter(|m| m.hidden)
        .collect::<Vec<_>>();
    let pages = all.len().div_ceil(PAGE_SIZE).max(1);
    let page = state.page.min(pages - 1);
    let mut c = Canvas::new();
    let title = format!(
        "{} <b>{}</b> · {}",
        icon("hide", ui.premium),
        ui.text("hidden_title", &[]),
        all.len()
    );
    c.add(title.clone(), title);
    let hint = format!(
        "<i>{}</i>",
        escape(&ui.text("hidden_hint", &[("prefix", prefix.into())]))
    );
    c.add(hint.clone(), hint);
    for module in all.iter().skip(page * PAGE_SIZE).take(PAGE_SIZE) {
        let (r, p) = entry(&mut c, module, state, ui);
        let undo = if owner {
            format!(
                " {}",
                c.tap(
                    ui.text("unhide_short", &[]),
                    Action::HelpToggle {
                        module: module.name.clone(),
                        state: state.clone()
                    }
                )
            )
        } else {
            String::new()
        };
        c.add(format!("{r}{undo}"), p);
    }
    if all.is_empty() {
        c.add(ui.text("hidden_none", &[]), ui.text("hidden_none", &[]));
    }
    let mut rows = vec![];
    if owner && !all.is_empty() {
        rows.push(vec![Key {
            text: ui.text("unhide_all", &[]),
            action: Some(Action::HelpUnhideAll {
                state: state.clone(),
            }),
            url: None,
        }]);
    }
    let navs = pager(state, page, pages, false);
    if !navs.is_empty() {
        rows.push(navs);
    }
    rows.push(vec![
        nav(
            format!("‹ {}", ui.text("back", &[])),
            State {
                hidden: false,
                page: 0,
                ..state.home()
            },
        ),
        close(ui),
    ]);
    Ok(c.finish(rows))
}
pub fn resolve<'a>(
    catalog: &'a Catalog,
    query: &str,
    prefix: &str,
    aliases: &HashMap<String, String>,
) -> Option<(&'a Module, bool)> {
    if let Some(module) = catalog.modules.iter().find(|m| {
        m.name
            .eq_ignore_ascii_case(crate::original::canonical_module(query))
    }) {
        return Some((module, true));
    }
    let query_cmd = query.strip_prefix(prefix).unwrap_or(query).to_lowercase();
    let resolved = crate::dispatcher::resolve(&query_cmd, "", aliases)
        .ok()
        .map(|(name, _)| name)
        .unwrap_or(query_cmd);
    if let Some(module) = catalog
        .modules
        .iter()
        .find(|m| m.commands.iter().any(|c| c.name == resolved))
    {
        return Some((module, true));
    }
    catalog
        .modules
        .iter()
        .map(|m| (similarity(&query.to_lowercase(), &m.name.to_lowercase()), m))
        .max_by(|(a, _), (b, _)| a.total_cmp(b))
        .map(|(_, m)| (m, false))
}
/// SequenceMatcher's matching-block ratio, including contiguous blocks rather than edit distance.
fn similarity(a: &str, b: &str) -> f64 {
    let a = a.chars().take(1024).collect::<Vec<_>>();
    let b = b.chars().take(128).collect::<Vec<_>>();
    fn matched(a: &[char], b: &[char]) -> usize {
        let mut best = (0, 0, 0);
        let mut previous = vec![0; b.len() + 1];
        for (i, ac) in a.iter().enumerate() {
            let mut next = vec![0; b.len() + 1];
            for (j, bc) in b.iter().enumerate() {
                if ac == bc {
                    next[j + 1] = previous[j] + 1;
                    if next[j + 1] > best.2 {
                        best = (i + 1 - next[j + 1], j + 1 - next[j + 1], next[j + 1]);
                    }
                }
            }
            previous = next;
        }
        if best.2 == 0 {
            0
        } else {
            best.2
                + matched(&a[..best.0], &b[..best.1])
                + matched(&a[best.0 + best.2..], &b[best.1 + best.2..])
        }
    }
    if a.is_empty() && b.is_empty() {
        1.0
    } else {
        2.0 * matched(&a, &b) as f64 / (a.len() + b.len()) as f64
    }
}
#[allow(clippy::too_many_arguments)] // Shares the same explicit presentation inputs as the catalogue renderer.
fn card(
    catalog: &Catalog,
    state: &State,
    query: &str,
    ui: Ui,
    prefix: &str,
    bot: &str,
    owner: bool,
    aliases: &HashMap<String, String>,
) -> Result<Screen> {
    if catalog
        .libraries
        .iter()
        .any(|s| s.eq_ignore_ascii_case(query))
    {
        let text = ui.text("library", &[("name", escape(query))]);
        return Ok(Screen {
            text: text.clone(),
            pages: vec![text],
            html: true,
            link_preview: false,
            ..Default::default()
        });
    }
    let Some((module, exact)) = resolve(catalog, query, prefix, aliases) else {
        let text = escape(&ui.text("missing", &[("name", query.into())]));
        return Ok(Screen {
            text,
            html: true,
            ..Default::default()
        });
    };
    let state = State {
        module: Some(module.name.clone()),
        closest: state.closest || !exact,
        ..state.clone()
    };
    let mut c = Canvas::new();
    if let Some(url) = &module.metadata.banner_url {
        c.rich.push(format!("<img src=\"{}\"/>", escape(url)));
    }
    let title = format!(
        "{} <b>{}</b>{} {}",
        icon(if module.core { "core" } else { "module" }, ui.premium),
        escape(&module.name),
        module
            .metadata
            .version
            .as_ref()
            .map(|v| format!(" {}", badge(&format!("v{v}"), "")))
            .unwrap_or_default(),
        badge(
            &ui.text(
                if module.core {
                    "builtin_badge"
                } else {
                    "external_badge"
                },
                &[]
            ),
            if module.core {
                ""
            } else {
                " style=\"success\""
            }
        )
    );
    c.add(
        title,
        format!(
            "<b>{}</b> · {}",
            escape(&module.name),
            ui.text(
                if module.core {
                    "builtin_badge"
                } else {
                    "external_badge"
                },
                &[]
            )
        ),
    );
    if !module.description.is_empty() {
        let description = format!(
            "<blockquote>{}</blockquote>",
            escape(&short(&module.description, 300))
        );
        c.add(description.clone(), description);
    }
    let commands = module
        .commands
        .iter()
        .filter(|c| c.text_allowed)
        .collect::<Vec<_>>();
    let inline_count = module.commands.iter().filter(|c| c.inline).count();
    let guest_count = module.commands.iter().filter(|c| c.guest).count();
    let page_size = if module.core {
        (commands.len() + inline_count + guest_count).max(1)
    } else {
        COMMAND_PAGE_SIZE
    };
    let pages = (commands.len() + inline_count + guest_count)
        .div_ceil(page_size)
        .max(1);
    let page = state.command_page.min(pages - 1);
    let mut rich = vec![];
    let mut plain = vec![];
    for cmd in commands.iter().skip(page * page_size).take(page_size) {
        let full = format!("{prefix}{}", cmd.name);
        let mut aliases = aliases
            .keys()
            .filter_map(|alias| {
                crate::dispatcher::resolve(alias, "", aliases)
                    .ok()
                    .filter(|(name, _)| name == &cmd.name)
                    .map(|_| format!("<code>{}</code>", escape(&format!("{prefix}{alias}"))))
            })
            .take(5)
            .collect::<Vec<_>>();
        for (alias, command) in [("cfg", "config"), ("fcfg", "fconfig"), ("eval", "e")] {
            let label = format!("<code>{}</code>", escape(&format!("{prefix}{alias}")));
            if cmd.name == command && !aliases.contains(&label) {
                aliases.push(label);
            }
        }
        let (usage, description) = parse_doc(&cmd.description);
        let line = format!(
            "<code>{}</code>{} — {}{}",
            escape(&full),
            if usage.is_empty() {
                String::new()
            } else {
                format!(" <i>{}</i>", escape(&short(&usage, 80)))
            },
            escape(&short(
                if description.is_empty() {
                    ui.text("undoc", &[])
                } else {
                    description
                }
                .as_str(),
                120
            )),
            if aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", aliases.join(", "))
            }
        );
        let copy = c.tap(format!("📋 {}", cmd.name), Action::Copy { text: full });
        rich.push(format!("{line} {copy}"));
        plain.push(line);
    }
    let title = format!(
        "{} <b>{}</b> · {}",
        icon("command", ui.premium),
        ui.text("commands", &[]),
        commands.len()
    );
    if rich.is_empty() {
        rich.push(ui.text("no_commands", &[]));
        plain.push(ui.text("no_commands", &[]));
    }
    c.add(
        details(&title, &rich.join("<br>"), commands.len() <= 8),
        title,
    );
    c.plain.extend(plain);
    for (guest, key) in [(false, "inline"), (true, "guest")] {
        let handlers = module
            .commands
            .iter()
            .filter(|c| if guest { c.guest } else { c.inline })
            .collect::<Vec<_>>();
        if handlers.is_empty() {
            continue;
        }
        let title = format!(
            "{} <b>{}</b> · {}",
            icon(if guest { "guest" } else { "bot" }, ui.premium),
            ui.text(key, &[]),
            handlers.len()
        );
        let lines = handlers
            .iter()
            .skip(
                (page * page_size)
                    .saturating_sub(commands.len() + if guest { inline_count } else { 0 }),
            )
            .take(
                ((page + 1) * page_size)
                    .saturating_sub(commands.len() + if guest { inline_count } else { 0 })
                    .min(page_size),
            )
            .map(|cmd| {
                format!(
                    "<code>@{} {}</code> — {}",
                    escape(bot),
                    escape(&cmd.name),
                    escape(&short(&cmd.description, 60))
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        c.add(
            details(&title, &lines.replace('\n', "<br>"), false),
            format!("{title}\n{lines}"),
        );
    }
    if module.name == crate::info::MODULE {
        let title = ui.text("placeholders", &[]);
        let list = crate::info::PLACEHOLDERS
            .iter()
            .map(|p| format!("<code>{{{p}}}</code>"))
            .collect::<Vec<_>>()
            .join(" ");
        c.add(details(&title, &list, false), format!("{title}\n{list}"));
    }
    if let Some(dev) = &module.metadata.developer {
        let text = escape(&ui.text("developer", &[("name", dev.clone())]));
        c.add(text.clone(), text);
    }
    if state.closest {
        let text = format!(
            "ℹ️ <i>{}</i>",
            escape(&ui.text("not_exact", &[("name", module.name.clone())]))
        );
        c.add(text.clone(), text);
    }
    let mut rows = vec![];
    let navs = pager(&state, page, pages, true);
    if !navs.is_empty() {
        rows.push(navs);
    }
    if owner {
        let mut row = vec![];
        if module.configurable {
            row.push(Key {
                text: format!("⚙ {}", ui.text("settings", &[])),
                action: Some(Action::HelpSettings {
                    module: module.name.clone(),
                    state: state.clone(),
                }),
                url: None,
            });
        }
        row.push(Key {
            text: ui.text(if module.hidden { "unhide" } else { "hide" }, &[]),
            action: Some(Action::HelpToggle {
                module: module.name.clone(),
                state: state.clone(),
            }),
            url: None,
        });
        rows.push(row);
    }
    rows.push(vec![
        nav(format!("‹ {}", ui.text("back", &[])), state.home()),
        close(ui),
    ]);
    Ok(c.finish(rows))
}

pub async fn build(
    ctx: &Context,
    user: i64,
    origin: Option<&Message>,
    state: &State,
) -> Result<Screen> {
    let owner = security::owners(&ctx.db, ctx.owner).contains(&user);
    let all = catalog(ctx);
    let (allowed, restricted) = visible(ctx, user, origin, &all).await?;
    let source = if owner && state.force && state.module.is_none() {
        &all
    } else {
        &allowed
    };
    let mut state = state.clone();
    if !owner {
        state.force = false;
    }
    let mut aliases: HashMap<String, String> =
        serde_json::from_value(ctx.db.get("Settings", "aliases")).unwrap_or_default();
    for (alias, command) in [("cfg", "config"), ("fcfg", "fconfig"), ("eval", "e")] {
        aliases
            .entry(alias.into())
            .or_insert_with(|| command.into());
    }
    let bot = ctx
        .bot
        .read()
        .await
        .as_ref()
        .map(|bot| bot.bot.username.clone())
        .unwrap_or_else(|| "bot".into());
    let banner = crate::info::Config::load(&ctx.db)?.help_banner_url;
    let prefix = ctx.db.prefix_for(Some(user), ctx.owner);
    render(
        source,
        &state,
        Ui {
            english: ctx.db.language().split_whitespace().next() == Some("en"),
            premium: ctx
                .db
                .get("RustRuntime", "premium")
                .as_bool()
                .unwrap_or(false),
        },
        &prefix,
        &bot,
        owner,
        restricted,
        &aliases,
        &banner,
    )
}
pub fn toggle_hidden(ctx: &Context, module: &str) -> Result<bool> {
    ensure!(
        catalog(ctx).modules.iter().any(|m| m.name == module),
        "module unloaded"
    );
    ctx.db.transaction(|data| {
        let ns = data
            .entry("atarustia.help")
            .or_insert_with(|| serde_json::json!({}));
        let array = ns
            .as_object_mut()
            .context("invalid help namespace")?
            .entry("hidden")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("invalid hidden list")?;
        if array.iter().any(|v| {
            v.as_str()
                .is_some_and(|n| crate::original::canonical_module(n) == module)
        }) {
            array.retain(|v| {
                !v.as_str()
                    .is_some_and(|n| crate::original::canonical_module(n) == module)
            });
            Ok(false)
        } else {
            array.push(module.into());
            Ok(true)
        }
    })
}
pub async fn send(ctx: &Context, message: &Message, args: &str) -> Result<()> {
    let user = message
        .sender_id()
        .context("help requires an identifiable caller")?
        .bare_id_unchecked();
    let state = State::parse(args)?;
    let screen = build(ctx, user, Some(message), &state).await?;
    if let Some(bot) = ctx.bot.read().await.clone()
        && bot.form(ctx, message, screen.clone()).await.is_ok()
    {
        return Ok(());
    }
    let pages = if screen.pages.is_empty() {
        vec![screen.text]
    } else {
        screen.pages
    };
    for (index, page) in pages.into_iter().enumerate() {
        let input = grammers_client::message::InputMessage::new()
            .html(page)
            .link_preview(false);
        if index == 0 && message.outgoing() {
            message.edit(input).await?;
        } else {
            message.reply(input).await?;
        }
    }
    Ok(())
}
pub async fn callback(
    ctx: &Context,
    query: &CallbackQuery,
    unit: &str,
    action: Action,
) -> Result<grammers_client::message::InputMessage> {
    let user = query.sender_id().bare_id_unchecked();
    let owner = security::owners(&ctx.db, ctx.owner).contains(&user);
    let gate = ctx.forms.lock().unwrap().gate(unit);
    let origin = gate.as_ref().map(|g| &g.message);
    let state = match action {
        Action::HelpNavigate { mut state } => {
            if state
                .module
                .as_ref()
                .is_some_and(|name| !catalog(ctx).modules.iter().any(|m| &m.name == name))
            {
                state = state.home();
            }
            state
        }
        Action::HelpToggle { module, state } => {
            ensure!(owner, "owner only help preferences");
            toggle_hidden(ctx, &module)?;
            state
        }
        Action::HelpUnhideAll { state } => {
            ensure!(owner, "owner only help preferences");
            ctx.db
                .set("atarustia.help", "hidden", serde_json::json!([]))?;
            state
        }
        Action::HelpSettings { module, state } => {
            ensure!(owner, "owner only configuration");
            ctx.forms
                .lock()
                .unwrap()
                .set_help_return(unit, Some(state))?;
            let screen = crate::screens::config_for_unit(ctx, &module, unit)?;
            return ctx.forms.lock().unwrap().replace(unit, screen);
        }
        Action::Help { module } => State {
            module,
            ..Default::default()
        },
        _ => anyhow::bail!("not a help action"),
    };
    ctx.forms.lock().unwrap().set_help_return(unit, None)?;
    let screen = build(ctx, user, origin, &state).await?;
    ctx.forms.lock().unwrap().replace(unit, screen)
}
