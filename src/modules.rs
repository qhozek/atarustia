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
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watcher {
    pub function: String,
    pub outgoing: Option<bool>,
    pub commands: Option<bool>,
    pub private: Option<bool>,
    pub contains: Option<String>,
    pub startswith: Option<String>,
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
            contains: None,
            startswith: None,
            sender: None,
            chat: None,
            event: "message".into(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub function: String,
    pub interval_seconds: u64,
    #[serde(default)]
    pub autostart: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
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
}
impl Watcher {
    pub fn accepts(&self, event: &Event) -> bool {
        self.event == event.kind
            && self.outgoing.is_none_or(|v| v == event.outgoing)
            && self.commands.is_none_or(|v| v == event.command)
            && self.private.is_none_or(|v| v == (event.chat > 0))
            && self
                .contains
                .as_ref()
                .is_none_or(|v| event.text.contains(v))
            && self
                .startswith
                .as_ref()
                .is_none_or(|v| event.text.starts_with(v))
            && self.sender.is_none_or(|v| event.sender == Some(v))
            && self.chat.is_none_or(|v| v == event.chat)
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
        }
        for task in &self.manifest.loops {
            ensure!(
                valid_name(&task.function) && (1..=86400).contains(&task.interval_seconds),
                "loop interval must be 1–86400 seconds"
            );
        }
        engine()
            .compile(&self.source)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(())
    }
    pub fn run(&self, command: &str, args: &str, config: serde_json::Value) -> Result<String> {
        let effect = self.run_command(command, args, config, serde_json::Value::Null)?;
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
}
