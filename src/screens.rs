//! Shared screen/storage adapters. The configurator itself lives in config_ui.
use crate::{commands::Context, forms::Screen};
use anyhow::{Context as _, Result};
use serde_json::Value;
pub fn help(ctx: &Context, module: Option<&str>) -> Result<Screen> {
    crate::help::render(
        &crate::help::catalog(ctx),
        &crate::help::State::parse(module.unwrap_or(""))?,
        crate::help::Ui {
            english: ctx.db.language().starts_with("en"),
            premium: crate::config_ui::ui(ctx).premium,
        },
        &ctx.db.prefix(),
        ctx.username.as_deref().unwrap_or("bot"),
        true,
        false,
        &serde_json::from_value(ctx.db.get("Settings", "aliases")).unwrap_or_default(),
        &crate::info::Config::load(&ctx.db)?.help_banner_url,
    )
}
pub fn config(ctx: &Context, args: &str) -> Result<Screen> {
    let args = crate::config_ui::arguments(args);
    crate::config_ui::build(
        ctx,
        &crate::config_ui::State {
            module: args.first().cloned(),
            parameter: args.get(1).cloned(),
            ..Default::default()
        },
        None,
    )
}
pub fn config_for_unit(ctx: &Context, module: &str, unit: &str) -> Result<Screen> {
    crate::config_ui::build(
        ctx,
        &crate::config_ui::State {
            module: (!module.is_empty()).then(|| module.into()),
            ..Default::default()
        },
        Some(unit),
    )
}
pub fn set_config(ctx: &Context, module: &str, key: &str, value: Value) -> Result<()> {
    let (mut config, _, validators) = config_data(ctx, module)?;
    let old = config.clone();
    let value = if let Some(v) = validators.get(key) {
        v.validate(crate::config_ui::config_value(value, Some(v)))?
    } else {
        value
    };
    let object = config
        .as_object_mut()
        .context("invalid module configuration")?;
    anyhow::ensure!(object.contains_key(key), "unknown config key");
    object.insert(key.into(), value);
    if module == crate::info::MODULE {
        serde_json::from_value::<crate::info::Config>(config.clone())?.validate()?;
    }
    if module == crate::update_ui::MODULE {
        let _: crate::update_ui::Config = serde_json::from_value(config.clone())?;
    }
    if module == "Tester" {
        serde_json::from_value::<crate::presentation::PingConfig>(config.clone())?.validate()?;
    }
    let notify = old != config && ctx.modules.lock().unwrap().package(module).is_some();
    ctx.db.transaction(|data| {
        data.entry(module)
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("invalid module namespace")?
            .insert("__config__".into(), config.clone());
        if notify {
            let changes = data
                .entry("RustModuleConfigChanges")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .context("invalid config queue")?;
            let old = changes
                .get(module)
                .and_then(|v| v.get("old"))
                .cloned()
                .unwrap_or(old);
            changes.insert(module.into(), serde_json::json!({"old":old,"new":config}));
        }
        Ok(())
    })
}
pub fn apply_config(
    ctx: &Context,
    module: &str,
    key: &str,
    step: Option<i64>,
    reset: bool,
) -> Result<()> {
    let (config, defaults, _) = config_data(ctx, module)?;
    let value = config.get(key).context("option removed")?;
    let value = if reset {
        defaults.get(key).context("no default value")?.clone()
    } else if let Some(step) = step {
        serde_json::json!(
            value
                .as_i64()
                .context("option is not an integer")?
                .checked_add(step)
                .context("integer overflow")?
        )
    } else {
        Value::Bool(!value.as_bool().context("option is not a boolean")?)
    };
    set_config(ctx, module, key, value)
}
type ConfigData = (
    Value,
    Value,
    std::collections::BTreeMap<String, crate::validators::Validator>,
);
pub fn config_data(ctx: &Context, module: &str) -> Result<ConfigData> {
    use crate::validators::Validator;
    if module == crate::update_ui::MODULE {
        return Ok((
            serde_json::to_value(crate::update_ui::Config::load(&ctx.db)?)?,
            serde_json::to_value(crate::update_ui::Config::default())?,
            std::collections::BTreeMap::from([("update_notifications".into(), Validator::Boolean)]),
        ));
    }
    if module == crate::info::MODULE {
        return Ok((
            serde_json::to_value(crate::info::Config::load(&ctx.db)?)?,
            serde_json::to_value(crate::info::Config::default())?,
            std::collections::BTreeMap::from([
                ("rich_mode".into(), Validator::Boolean),
                ("quote_media".into(), Validator::Boolean),
                ("invert_media".into(), Validator::Boolean),
            ]),
        ));
    }
    if module == "Settings" {
        let defaults = serde_json::json!({"allow_nonstandart_prefixes":false,"alias_emoji":"<tg-emoji emoji-id=4974259868996207180>▪️</tg-emoji>"});
        let mut values = defaults.clone();
        if let Some(saved) = ctx.db.get("Settings", "__config__").as_object() {
            for (k, v) in saved {
                values[k] = v.clone();
            }
        }
        return Ok((
            values,
            defaults,
            std::collections::BTreeMap::from([(
                "allow_nonstandart_prefixes".into(),
                Validator::Boolean,
            )]),
        ));
    }
    if module == "Tester" {
        return Ok((
            serde_json::to_value(crate::presentation::PingConfig::load(&ctx.db)?)?,
            serde_json::to_value(crate::presentation::PingConfig::default())?,
            std::collections::BTreeMap::from([
                ("rich_mode".into(), Validator::Boolean),
                ("quote_media".into(), Validator::Boolean),
                ("invert_media".into(), Validator::Boolean),
                (
                    "custom_message".into(),
                    Validator::String {
                        min_len: None,
                        max_len: Some(16384),
                    },
                ),
                (
                    "ping_emoji".into(),
                    Validator::String {
                        min_len: None,
                        max_len: Some(64),
                    },
                ),
            ]),
        ));
    }
    let package = ctx
        .modules
        .lock()
        .unwrap()
        .export()
        .into_iter()
        .find(|p| p.manifest.name == module)
        .context("unknown module")?;
    let mut values = package.manifest.config.clone();
    if let Some(saved) = ctx.db.get(module, "__config__").as_object() {
        for (k, v) in saved {
            values[k] = v.clone();
        }
    }
    Ok((values, package.manifest.config, package.manifest.validators))
}
