//! Native scripting API: bounded Rhai, no Python interpreter or implicit native dependencies.
use crate::{security::Permission, storage::atomic_write};
use anyhow::{Context, Result, ensure};
use rhai::{Dynamic, Engine, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MAX_SOURCE: usize = 512 * 1024;
pub fn engine() -> Engine {
    let mut e = Engine::new();
    e.set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new());
    e.set_max_modules(0);
    e.set_max_operations(100_000);
    e.set_max_expr_depths(64, 64);
    e.set_max_string_size(128 * 1024);
    e.set_max_array_size(4096);
    e.set_max_map_size(1024);
    e.set_max_call_levels(32);
    e.on_print(|_| {});
    e.on_debug(|_, _, _| {});
    e
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub function: String,
    #[serde(default)]
    pub permission: Permission,
    #[serde(default)]
    pub inline: bool,
    #[serde(default)]
    pub guest: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub watchers: Vec<Watcher>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    #[serde(default)]
    pub libraries: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub validators: std::collections::BTreeMap<String, crate::validators::Validator>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<crate::help::Metadata>,
    /// Runtime requirements checked before install: ffmpeg, inline, shell.
    #[serde(default)]
    pub scope: Vec<String>,
    /// Module strings: language → key → text, exposed to scripts as `strings`.
    #[serde(default)]
    pub strings: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// HTTPS JSON pack ({language: {key: text}}) fetched into `strings` on install.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packurl: Option<String>,
}
impl Manifest {
    /// Strings of `language` merged over the English fallback.
    pub fn strings_for(&self, language: &str) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        for (key, value) in self.strings.get("en").into_iter().flatten() {
            map.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        for (key, value) in self.strings.get(language).into_iter().flatten() {
            map.insert(key.clone(), serde_json::Value::String(value.clone()));
        }
        serde_json::Value::Object(map)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watcher {
    pub function: String,
    /// Filter values are tri-state: `null` accepts everything, `true`/`false`
    /// keeps only matching/non-matching events.
    pub outgoing: Option<bool>,
    pub commands: Option<bool>,
    pub private: Option<bool>,
    pub groups: Option<bool>,
    pub channels: Option<bool>,
    pub forwards: Option<bool>,
    pub reply: Option<bool>,
    pub mention: Option<bool>,
    pub inline: Option<bool>,
    pub media: Option<bool>,
    pub photos: Option<bool>,
    pub videos: Option<bool>,
    pub audios: Option<bool>,
    pub docs: Option<bool>,
    pub stickers: Option<bool>,
    pub contains: Option<String>,
    pub startswith: Option<String>,
    pub endswith: Option<String>,
    pub regex: Option<String>,
    pub sender: Option<i64>,
    pub chat: Option<i64>,
    pub event: String,
}
impl Default for Watcher {
    fn default() -> Self {
        Self {
            function: String::new(),
            outgoing: None,
            commands: None,
            private: None,
            groups: None,
            channels: None,
            forwards: None,
            reply: None,
            mention: None,
            inline: None,
            media: None,
            photos: None,
            videos: None,
            audios: None,
            docs: None,
            stickers: None,
            contains: None,
            startswith: None,
            endswith: None,
            regex: None,
            sender: None,
            chat: None,
            event: "message".into(),
        }
    }
}
impl Watcher {
    pub fn accepts(&self, event: &Event) -> bool {
        self.event == event.kind
            && self.outgoing.is_none_or(|v| v == event.outgoing)
            && self.commands.is_none_or(|v| v == event.command)
            && self
                .private
                .is_none_or(|v| v == (event.chat_kind == "private"))
            && self
                .groups
                .is_none_or(|v| v == matches!(event.chat_kind.as_str(), "group" | "supergroup"))
            && self
                .channels
                .is_none_or(|v| v == (event.chat_kind == "channel"))
            && self.forwards.is_none_or(|v| v == event.forward)
            && self.reply.is_none_or(|v| v == event.reply)
            && self.mention.is_none_or(|v| v == event.mention)
            && self.inline.is_none_or(|v| v == event.via_bot)
            && self.media.is_none_or(|v| v == event.media.is_some())
            && media_tag(self.photos, event, "photo")
            && media_tag(self.videos, event, "video")
            && media_tag(self.audios, event, "audio")
            && media_tag(self.docs, event, "document")
            && media_tag(self.stickers, event, "sticker")
            && self
                .contains
                .as_ref()
                .is_none_or(|v| event.text.contains(v))
            && self
                .startswith
                .as_ref()
                .is_none_or(|v| event.text.starts_with(v))
            && self
                .endswith
                .as_ref()
                .is_none_or(|v| event.text.ends_with(v))
            && self
                .regex
                .as_ref()
                .is_none_or(|v| regex::Regex::new(v).is_ok_and(|re| re.is_match(&event.text)))
            && self.sender.is_none_or(|v| event.sender == Some(v))
            && self.chat.is_none_or(|v| dialog_matches(v, event.chat))
    }
}
/// `true`/`false` keeps only/excludes messages of exactly this media kind.
fn media_tag(expected: Option<bool>, event: &Event, kind: &str) -> bool {
    expected.is_none_or(|v| v == (event.media.as_deref() == Some(kind)))
}
/// Compares a filter id with a Bot API dialog id, accepting bare ids too
/// (`-100…` and `…` both match a channel, like the Python original).
fn dialog_matches(filter: i64, chat: i64) -> bool {
    filter == chat || bare_dialog_id(filter) == bare_dialog_id(chat)
}
fn bare_dialog_id(id: i64) -> i64 {
    if id < -1_000_000_000_000 {
        -id - 1_000_000_000_000
    } else if id < 0 {
        -id
    } else {
        id
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub function: String,
    pub interval_seconds: u64,
    #[serde(default)]
    pub autostart: bool,
    /// Sleep before the first iteration instead of running it immediately.
    #[serde(default)]
    pub wait_before: bool,
    /// Database key set to `true` on loop start; the loop stops once it is `false`.
    #[serde(default)]
    pub stop_clause: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Event {
    pub text: String,
    pub chat: i64,
    pub sender: Option<i64>,
    pub outgoing: bool,
    pub command: bool,
    pub edited: bool,
    pub id: i32,
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
    /// Watcher-filter facts, precomputed from the converted message.
    #[serde(default)]
    pub chat_kind: String,
    #[serde(default)]
    pub forward: bool,
    #[serde(default)]
    pub reply: bool,
    #[serde(default)]
    pub mention: bool,
    #[serde(default)]
    pub via_bot: bool,
    #[serde(default)]
    pub media: Option<String>,
}
impl Event {
    /// Fills the watcher-filter fields from a converted Bot API message.
    pub fn enrich_from_botapi(&mut self, message: &crate::botapi::Message) {
        self.chat_kind = message.chat.kind.clone();
        self.forward = message.forward_origin.is_some();
        self.reply = message.reply_to_message_id.is_some();
        self.via_bot = message.via_bot.is_some();
        self.mention = message
            .entities
            .iter()
            .chain(message.caption_entities.iter())
            .any(|entity| matches!(entity.kind.as_str(), "mention" | "text_mention"));
        self.media = message.media_type.map(|kind| kind.name().to_owned());
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Package {
    pub manifest: Manifest,
    pub source: String,
    #[serde(default)]
    pub origin: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    digest: String,
    package: Package,
}
pub struct Registry {
    root: PathBuf,
    packages: BTreeMap<String, Package>,
}
impl Registry {
    pub fn pending(&self, hash: &str) -> Result<Package> {
        ensure!(
            hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()),
            "invalid revision hash"
        );
        let pending: Pending = serde_json::from_slice(&std::fs::read(
            self.root.join("pending").join(format!("{hash}.module")),
        )?)?;
        ensure!(
            pending.digest == hash && digest(&serde_json::to_vec(&pending.package)?) == hash,
            "pending snapshot corrupted"
        );
        Ok(pending.package)
    }
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
impl Package {
    pub fn validate(&self) -> Result<()> {
        if let Some(metadata) = &self.manifest.help {
            metadata.validate()?;
        }
        ensure!(
            valid_name(&self.manifest.name),
            "module name must be lowercase ASCII letters, digits or _"
        );
        ensure!(
            (!self.manifest.commands.is_empty()
                || !self.manifest.watchers.is_empty()
                || !self.manifest.loops.is_empty())
                && self.manifest.commands.len() <= 64
                && self.manifest.watchers.len() <= 32
                && self.manifest.loops.len() <= 16,
            "module needs handlers (maximum 64 commands, 32 watchers, 16 loops)"
        );
        ensure!(self.source.len() <= MAX_SOURCE, "module too large");
        for scope in &self.manifest.scope {
            ensure!(
                matches!(scope.as_str(), "ffmpeg" | "inline" | "shell"),
                "unknown scope requirement: {scope}"
            );
        }
        ensure!(
            self.manifest.strings.len() <= 8
                && serde_json::to_vec(&self.manifest.strings)?.len() <= 64 * 1024,
            "module strings exceed limits"
        );
        if let Some(url) = &self.manifest.packurl {
            let parsed = reqwest::Url::parse(url)?;
            ensure!(
                url.len() <= 512
                    && parsed.scheme() == "https"
                    && parsed.username().is_empty()
                    && parsed.password().is_none(),
                "packurl must be a plain HTTPS URL"
            );
        }
        for (key, validator) in &self.manifest.validators {
            let value = self
                .manifest
                .config
                .get(key)
                .context("validator has no config default")?;
            validator
                .validate(value.clone())
                .with_context(|| format!("invalid default for {key}"))?;
        }
        ensure!(
            self.manifest.libraries.len() <= 16
                && self
                    .manifest
                    .libraries
                    .values()
                    .map(String::len)
                    .sum::<usize>()
                    <= MAX_SOURCE,
            "libraries exceed limits"
        );
        for (name, source) in &self.manifest.libraries {
            ensure!(valid_name(name), "invalid library name");
            engine()
                .compile(source)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        let mut seen = std::collections::HashSet::new();
        for c in &self.manifest.commands {
            c.permission.validate()?;
            ensure!(
                valid_name(&c.name) && valid_name(&c.function) && seen.insert(&c.name),
                "invalid or duplicate command"
            );
            ensure!(
                !crate::commands::is_builtin(&c.name),
                "module cannot replace a built-in command"
            );
        }
        for watcher in &self.manifest.watchers {
            ensure!(
                valid_name(&watcher.function)
                    && matches!(
                        watcher.event.as_str(),
                        "message"
                            | "bot_message"
                            | "guest"
                            | "raw"
                            | "edited_message"
                            | "business_message"
                            | "edited_business_message"
                            | "business_connection"
                            | "deleted_business_messages"
                            | "message_reaction"
                            | "message_reaction_count"
                            | "poll"
                            | "poll_answer"
                            | "chat_join_request"
                            | "chat_boost"
                            | "purchased_paid_media"
                            | "pre_checkout_query"
                            | "shipping_query"
                            | "chat_member"
                            | "my_chat_member"
                            | "channel_post"
                            | "edited_channel_post"
                    ),
                "invalid watcher"
            );
            for filter in [&watcher.contains, &watcher.startswith, &watcher.endswith] {
                ensure!(
                    filter.as_ref().is_none_or(|v| v.len() <= 512),
                    "watcher text filter too long"
                );
            }
            if let Some(pattern) = &watcher.regex {
                ensure!(pattern.len() <= 512, "watcher regex too long");
                regex::Regex::new(pattern)
                    .map_err(|e| anyhow::anyhow!("invalid watcher regex: {e}"))?;
            }
        }
        for task in &self.manifest.loops {
            ensure!(
                valid_name(&task.function) && (1..=86400).contains(&task.interval_seconds),
                "loop interval must be 1–86400 seconds"
            );
            ensure!(
                task.stop_clause
                    .as_ref()
                    .is_none_or(|clause| valid_name(clause)),
                "loop stop_clause must be a valid key name"
            );
        }
        engine()
            .compile(&self.source)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(())
    }
    pub fn run(&self, command: &str, args: &str, config: serde_json::Value) -> Result<String> {
        let effect = self.run_command(command, args, config, serde_json::Value::Null, "en")?;
        Ok(effect
            .text
            .or_else(|| effect.screen.map(|s| s.text))
            .unwrap_or_default())
    }
    fn engine(&self) -> Result<Engine> {
        let mut e = engine();
        if !self.manifest.libraries.is_empty() {
            let mut resolver = rhai::module_resolvers::StaticModuleResolver::new();
            for (name, source) in &self.manifest.libraries {
                let ast = e
                    .compile(source)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                let module = rhai::Module::eval_ast_as_new(Scope::new(), &ast, &e)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                resolver.insert(name, module);
            }
            e.set_max_modules(16);
            e.set_module_resolver(resolver);
        }
        Ok(e)
    }
    pub fn run_command(
        &self,
        command: &str,
        args: &str,
        config: serde_json::Value,
        state: serde_json::Value,
        language: &str,
    ) -> Result<Effect> {
        let cmd = self
            .manifest
            .commands
            .iter()
            .find(|c| c.name == command)
            .context("unknown module command")?;
        let e = self.engine()?;
        let ast = e
            .compile(&self.source)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let config = rhai::serde::to_dynamic(config).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let mut scope = Scope::new();
        scope.push(
            "state",
            rhai::serde::to_dynamic(state).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        scope.push(
            "strings",
            rhai::serde::to_dynamic(self.manifest.strings_for(language))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let result = e
            .call_fn::<Dynamic>(&mut scope, &ast, &cmd.function, (args.to_owned(), config))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        effect(result)
    }
    pub fn run_event(
        &self,
        function: &str,
        event: serde_json::Value,
        config: serde_json::Value,
        state: serde_json::Value,
        language: &str,
    ) -> Result<Effect> {
        let e = self.engine()?;
        let ast = e
            .compile(&self.source)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let mut scope = Scope::new();
        scope.push(
            "state",
            rhai::serde::to_dynamic(state).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        scope.push(
            "strings",
            rhai::serde::to_dynamic(self.manifest.strings_for(language))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        );
        let event = rhai::serde::to_dynamic(event).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let config = rhai::serde::to_dynamic(config).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let result = e
            .call_fn::<Dynamic>(&mut scope, &ast, function, (event, config))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        effect(result)
    }
}
fn effect(result: Dynamic) -> Result<Effect> {
    if result.is_unit() {
        return Ok(Effect::default());
    }
    if !result.is_map() {
        return Ok(Effect {
            text: Some(result.to_string()),
            ..Default::default()
        });
    }
    let result =
        rhai::serde::from_dynamic::<Effect>(&result).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    ensure!(
        result.text.as_ref().is_none_or(|t| t.len() <= 128 * 1024),
        "script output too large"
    );
    if let Some(screen) = &result.screen {
        screen.validate()?;
    }
    ensure!(
        !(result.screen.is_some() && (result.media.is_some() || !result.album.is_empty())),
        "return a screen or direct media, not both"
    );
    ensure!(
        result.media.is_none() || result.album.is_empty(),
        "choose media or album"
    );
    if let Some(media) = &result.media {
        media.validate()?;
    }
    if !result.album.is_empty() {
        crate::media::Media::validate_album(&result.album)?;
    }
    if let Some(state) = &result.state {
        ensure!(
            serde_json::to_vec(state)?.len() <= 256 * 1024,
            "module state exceeds 256 KiB"
        );
    }
    ensure!(result.frames.len() <= 100, "too many animation frames");
    if !result.frames.is_empty() {
        ensure!(
            result.frame_ms == 0 || (100..=10_000).contains(&result.frame_ms),
            "frame_ms must be within 100–10000"
        );
        ensure!(
            result.frames.iter().all(|f| f.chars().count() <= 4096),
            "animation frame exceeds the message limit"
        );
        ensure!(
            result.screen.is_none() && result.media.is_none() && result.album.is_empty(),
            "frames cannot combine with a screen or media"
        );
    }
    Ok(result)
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Effect {
    pub text: Option<String>,
    pub state: Option<serde_json::Value>,
    pub screen: Option<crate::forms::Screen>,
    pub media: Option<crate::media::Media>,
    pub album: Vec<crate::media::Media>,
    /// Message animation: every frame is sent as an edit of the previous one.
    #[serde(default)]
    pub frames: Vec<String>,
    /// Delay between frames, milliseconds. `0` means the default of 1000.
    #[serde(default)]
    pub frame_ms: u32,
}
impl Registry {
    pub fn load(root: &Path, owner: i64) -> Result<Self> {
        let root = root.join("loaded_modules").join(owner.to_string());
        std::fs::create_dir_all(&root)?;
        let mut r = Self {
            root,
            packages: BTreeMap::new(),
        };
        for file in std::fs::read_dir(&r.root)? {
            let path = file?.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let package: Package = serde_json::from_slice(&std::fs::read(&path)?)
                .with_context(|| format!("invalid module {}", path.display()))?;
            package.validate()?;
            ensure!(
                path.file_stem().and_then(|s| s.to_str()) == Some(package.manifest.name.as_str()),
                "module filename mismatch"
            );
            r.ensure_unique(&package)?;
            r.packages.insert(package.manifest.name.clone(), package);
        }
        Ok(r)
    }
    fn ensure_unique(&self, p: &Package) -> Result<()> {
        for existing in self
            .packages
            .values()
            .filter(|x| x.manifest.name != p.manifest.name)
        {
            for c in &p.manifest.commands {
                ensure!(
                    !existing.manifest.commands.iter().any(|x| x.name == c.name),
                    "command {} already installed",
                    c.name
                );
            }
        }
        Ok(())
    }
    pub fn list(&self) -> Vec<Manifest> {
        self.packages.values().map(|p| p.manifest.clone()).collect()
    }
    pub fn command(&self, name: &str) -> Option<(Package, Command)> {
        self.packages.values().find_map(|p| {
            p.manifest
                .commands
                .iter()
                .find(|c| c.name == name)
                .map(|c| (p.clone(), c.clone()))
        })
    }
    pub fn stage(&self, package: Package) -> Result<String> {
        package.validate()?;
        self.ensure_unique(&package)?;
        let hash = digest(&serde_json::to_vec(&package)?);
        atomic_write(
            &self.root.join("pending").join(format!("{hash}.module")),
            &serde_json::to_vec_pretty(&Pending {
                digest: hash.clone(),
                package,
            })?,
        )?;
        Ok(hash)
    }
    pub fn approve(&mut self, hash: &str) -> Result<()> {
        ensure!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid approval hash"
        );
        let path = self.root.join("pending").join(format!("{hash}.module"));
        let pending: Pending = serde_json::from_slice(&std::fs::read(&path)?)?;
        ensure!(
            hash.len() == 64
                && hash == pending.digest
                && hash == digest(&serde_json::to_vec(&pending.package)?),
            "approval hash mismatch"
        );
        pending.package.validate()?;
        self.ensure_unique(&pending.package)?;
        let name = pending.package.manifest.name.clone();
        atomic_write(
            &self.root.join(format!("{name}.json")),
            &serde_json::to_vec_pretty(&pending.package)?,
        )?;
        self.packages.insert(name, pending.package);
        std::fs::remove_file(path)?;
        Ok(())
    }
    pub fn remove(&mut self, name: &str) -> Result<()> {
        ensure!(valid_name(name), "invalid name");
        ensure!(self.packages.contains_key(name), "module not installed");
        std::fs::remove_file(self.root.join(format!("{name}.json")))?;
        self.packages.remove(name);
        Ok(())
    }
    pub fn export(&self) -> Vec<Package> {
        self.packages.values().cloned().collect()
    }
    pub fn clear(&mut self) -> Result<()> {
        let names: Vec<_> = self.packages.keys().cloned().collect();
        for name in names {
            self.remove(&name)?;
        }
        let pending = self.root.join("pending");
        if pending.is_dir() {
            for entry in std::fs::read_dir(pending)? {
                let path = entry?.path();
                if path.is_file() && path.extension().is_some_and(|e| e == "module") {
                    std::fs::remove_file(path)?;
                }
            }
        }
        Ok(())
    }
    pub async fn updates(packages: Vec<Package>) -> Vec<(String, String, Package)> {
        let mut updates = vec![];
        for installed in packages {
            let Some(url) = &installed.origin else {
                continue;
            };
            if let Ok(remote) = download(url).await
                && (remote.source != installed.source
                    || serde_json::to_value(&remote.manifest).ok()
                        != serde_json::to_value(&installed.manifest).ok())
            {
                updates.push((installed.manifest.name, url.clone(), remote));
            }
        }
        updates
    }
}
pub async fn download(url: &str) -> Result<Package> {
    let parsed = reqwest::Url::parse(url)?;
    ensure!(parsed.scheme() == "https", "module URL must use HTTPS");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let mut response = client.get(parsed).send().await?.error_for_status()?;
    ensure!(
        response.url().scheme() == "https",
        "redirect to insecure HTTP refused"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= MAX_SOURCE, "module too large");
        bytes.extend_from_slice(&chunk);
    }
    let mut package: Package = serde_json::from_slice(&bytes).context(
        "expected a Rust/Rhai JSON module package; Python .py modules are not compatible",
    )?;
    package.origin = Some(url.to_owned());
    package.validate()?;
    Ok(package)
}
/// Downloads the language pack of `packurl` into `package.manifest.strings`.
/// Fetched on install, so the module never reaches for the network at runtime.
pub async fn resolve_packurl(package: &mut Package) -> Result<()> {
    let Some(url) = package.manifest.packurl.clone() else {
        return Ok(());
    };
    let parsed = reqwest::Url::parse(&url)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let mut response = client.get(parsed).send().await?.error_for_status()?;
    ensure!(
        response.url().scheme() == "https",
        "redirect to insecure HTTP refused"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 64 * 1024,
            "language pack too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let pack: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_slice(&bytes).context("packurl must be JSON: {language: {key: text}}")?;
    package.manifest.strings = pack;
    package.validate()
}
pub fn evaluate(source: &str) -> Result<String> {
    ensure!(source.len() <= MAX_SOURCE, "script too large");
    engine()
        .eval::<Dynamic>(source)
        .map(|d| d.to_string())
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn package() -> Package {
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap()
    }
    #[test]
    fn install_requires_exact_approval() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Registry::load(d.path(), 1).unwrap();
        let p = package();
        let hash = r.stage(p).unwrap();
        assert!(r.command("hello").is_none());
        assert!(r.approve("bad").is_err());
        r.approve(&hash).unwrap();
        let (p, _) = r.command("hello").unwrap();
        assert_eq!(
            p.run("hello", "world", p.manifest.config.clone()).unwrap(),
            "Hello, world!"
        );
        assert_eq!(Registry::load(d.path(), 1).unwrap().list().len(), 1);
        r.remove("hello").unwrap();
        assert!(r.list().is_empty());
    }
    #[test]
    fn limits_and_no_native_io() {
        assert!(evaluate("loop { }").is_err());
        assert!(evaluate("read_file(\"secret\")").is_err());
        assert!(evaluate("import \"local\" as secret; secret::value()").is_err());
        assert_eq!(evaluate("40+2").unwrap(), "42");
    }
    #[test]
    fn builtin_collision() {
        let mut p = package();
        p.manifest.commands[0].name = "terminal".into();
        assert!(p.validate().is_err());
    }
    #[test]
    fn validation_does_not_execute_library_code() {
        let mut p = package();
        p.manifest.libraries.insert(
            "untrusted".into(),
            "throw \"must not run during validation\";".into(),
        );
        p.validate().unwrap();
        assert!(p.run("hello", "", serde_json::json!({})).is_err());
    }
    #[test]
    fn invalid_default_cannot_be_installed() {
        let mut p = package();
        p.manifest.validators.insert(
            "greeting".into(),
            crate::validators::Validator::Integer {
                min: Some(0),
                max: None,
            },
        );
        assert!(p.validate().is_err());
    }
    #[test]
    fn multiple_pending_sources_are_preserved() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Registry::load(d.path(), 1).unwrap();
        let p = package();
        let a = r.stage(p.clone()).unwrap();
        let mut next = p;
        next.source.push_str("\n// a distinct pinned revision\n");
        let b = r.stage(next).unwrap();
        assert_ne!(a, b);
        r.approve(&a).unwrap();
        r.approve(&b).unwrap();
    }
    #[test]
    fn failed_package_does_not_replace_running_module() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Registry::load(d.path(), 1).unwrap();
        let p = package();
        let h = r.stage(p.clone()).unwrap();
        r.approve(&h).unwrap();
        let mut invalid = p;
        invalid.source = "this is not Rhai }".into();
        assert!(r.stage(invalid).is_err());
        assert!(r.command("hello").is_some());
    }
    fn event(json: serde_json::Value) -> Event {
        serde_json::from_value(json).unwrap()
    }
    fn watcher(json: serde_json::Value) -> Watcher {
        serde_json::from_value(json).unwrap()
    }
    fn outgoing_photo_in_group() -> Event {
        event(serde_json::json!({
            "text": "look", "chat": -1001234567890i64, "sender": 7, "outgoing": true,
            "command": false, "edited": false, "id": 5, "kind": "message",
            "chat_kind": "supergroup", "forward": false, "reply": false,
            "mention": false, "via_bot": false, "media": "photo"
        }))
    }
    #[test]
    fn watcher_tag_matrix() {
        let base = outgoing_photo_in_group();
        for (filter, expected) in [
            (r#"{"outgoing":true}"#, true),
            (r#"{"outgoing":false}"#, false),
            (r#"{"commands":false}"#, true),
            (r#"{"commands":true}"#, false),
            (r#"{"groups":true}"#, true),
            (r#"{"groups":false}"#, false),
            (r#"{"private":true}"#, false),
            (r#"{"channels":true}"#, false),
            (r#"{"photos":true}"#, true),
            (r#"{"photos":false}"#, false),
            (r#"{"videos":true}"#, false),
            (r#"{"videos":false}"#, true),
            (r#"{"media":true}"#, true),
            (r#"{"media":false}"#, false),
            (r#"{"contains":"oo"}"#, true),
            (r#"{"contains":"zz"}"#, false),
            (r#"{"startswith":"look"}"#, true),
            (r#"{"endswith":"ok"}"#, true),
            (r#"{"regex":"l[o]{2}k"}"#, true),
            (r#"{"regex":"^nope"}"#, false),
            (r#"{"sender":7}"#, true),
            (r#"{"sender":8}"#, false),
            (r#"{"chat":-1001234567890}"#, true),
            // Bare channel ids match the -100 form, like the Python original
            (r#"{"chat":1234567890}"#, true),
            (r#"{"mention":true}"#, false),
            (r#"{"reply":true}"#, false),
            (r#"{"forwards":false}"#, true),
            (r#"{"inline":false}"#, true),
        ] {
            let mut filter: serde_json::Value =
                serde_json::from_str(filter).expect("valid filter JSON");
            filter["event"] = "message".into();
            filter["function"] = "w".into();
            assert_eq!(
                watcher(filter.clone()).accepts(&base),
                expected,
                "filter {filter} against an outgoing photo in a group"
            );
        }
    }
    #[test]
    fn invalid_watcher_regex_is_rejected() {
        let mut p = package();
        p.manifest.watchers.push(watcher(
            serde_json::json!({"function":"w","event":"message","regex":"("}),
        ));
        assert!(p.validate().is_err());
    }
    #[test]
    fn stop_clause_must_be_a_valid_key() {
        let mut p = package();
        p.manifest.loops.push(Loop {
            function: "tick".into(),
            interval_seconds: 5,
            autostart: true,
            wait_before: false,
            stop_clause: Some("not a key!".into()),
        });
        assert!(p.validate().is_err());
    }
    #[test]
    fn unknown_scope_requirement_is_rejected() {
        let mut p = package();
        p.manifest.scope.push("root".into());
        assert!(p.validate().is_err());
    }
    #[test]
    fn strings_reach_the_script_per_language() {
        let mut p = package();
        p.manifest
            .strings
            .entry("en".into())
            .or_default()
            .insert("greeting".into(), "Hello".into());
        p.manifest
            .strings
            .entry("ru".into())
            .or_default()
            .insert("greeting".into(), "Привет".into());
        p.source = "fn hello(args, config) { strings.greeting }".into();
        p.validate().unwrap();
        assert_eq!(
            p.run_command(
                "hello",
                "",
                serde_json::json!({}),
                serde_json::Value::Null,
                "ru"
            )
            .unwrap()
            .text
            .unwrap(),
            "Привет"
        );
        // Unknown languages fall back to English
        assert_eq!(
            p.run_command(
                "hello",
                "",
                serde_json::json!({}),
                serde_json::Value::Null,
                "de"
            )
            .unwrap()
            .text
            .unwrap(),
            "Hello"
        );
    }
    #[test]
    fn frames_are_validated_and_exclusive() {
        let mut p = package();
        p.source = "fn hello(args, config) { #{frames:[\"a\",\"b\"],frame_ms:50} }".into();
        p.validate().unwrap();
        assert!(
            p.run_command(
                "hello",
                "",
                serde_json::json!({}),
                serde_json::Value::Null,
                "en"
            )
            .is_err()
        );
        p.source = "fn hello(args, config) { #{frames:[\"a\"],screen:#{text:\"x\"}} }".into();
        assert!(
            p.run_command(
                "hello",
                "",
                serde_json::json!({}),
                serde_json::Value::Null,
                "en"
            )
            .is_err()
        );
    }
    #[test]
    fn tags_example_validates_and_animates() {
        let p: Package =
            serde_json::from_str(include_str!("../examples/tags.module.json")).unwrap();
        p.validate().unwrap();
        let effect = p
            .run_command(
                "pulse",
                "",
                p.manifest.config.clone(),
                serde_json::Value::Null,
                "en",
            )
            .unwrap();
        assert_eq!(effect.frames.len(), 3);
        assert_eq!(effect.frame_ms, 300);
        assert!(effect.frames[0].contains("Photo saved"));
        // The Russian pack takes over for the ru language
        let ru = p
            .run_command(
                "pulse",
                "",
                p.manifest.config.clone(),
                serde_json::Value::Null,
                "ru",
            )
            .unwrap();
        assert!(ru.frames[0].contains("Фото сохранено"));
    }
}
