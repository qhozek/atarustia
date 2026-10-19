//! Shared UI construction: Telegram transport is kept outside screen rendering.
use crate::{
    commands::Context,
    forms::{Action, Key, Screen},
    output::escape,
};
use anyhow::{Context as _, Result};
use serde_json::Value;
fn button(text: impl Into<String>, action: Action) -> Key {
    Key {
        text: text.into(),
        action: Some(action),
        url: None,
    }
}
pub fn help(ctx: &Context, module: Option<&str>) -> Result<Screen> {
    if let Some(name) = module {
        let p = ctx
            .modules
            .lock()
            .unwrap()
            .export()
            .into_iter()
            .find(|p| p.manifest.name == name)
            .context("unknown module")?;
        let mut text = format!(
            "<b>{}</b>\n{}\n\n",
            escape(name),
            escape(&p.manifest.description)
        );
        for cmd in p.manifest.commands {
            text.push_str(&format!(
                "<code>{}{}</code> — {}\n",
                escape(&ctx.db.prefix()),
                escape(&cmd.name),
                escape(&cmd.description)
            ));
        }
        return with_help_banner(
            ctx,
            Screen {
                text,
                html: true,
                buttons: vec![vec![
                    button(
                        ctx.tr("config", &[]),
                        Action::ConfigOpen {
                            module: name.into(),
                        },
                    ),
                    button(ctx.tr("back", &[]), Action::Help { module: None }),
                ]],
                ..Default::default()
            },
        );
    }
    let mut rows = vec![];
    for module in ctx.modules.lock().unwrap().list() {
        if !crate::preferences::hidden(&ctx.db, &module.name) {
            rows.push(vec![button(
                module.name.clone(),
                Action::Help {
                    module: Some(module.name),
                },
            )]);
        }
    }
    with_help_banner(
        ctx,
        Screen {
            text: ctx.help(),
            buttons: rows.into_iter().take(30).collect(),
            ..Default::default()
        },
    )
}
pub fn config(ctx: &Context, module: &str) -> Result<Screen> {
    if module.is_empty() {
        let mut rows = vec![
            vec![button(
                crate::update_ui::MODULE,
                Action::ConfigOpen {
                    module: crate::update_ui::MODULE.into(),
                },
            )],
            vec![button(
                crate::info::MODULE,
                Action::ConfigOpen {
                    module: crate::info::MODULE.into(),
                },
            )],
        ];
        for m in ctx.modules.lock().unwrap().list() {
            rows.push(vec![button(
                m.name.clone(),
                Action::ConfigOpen { module: m.name },
            )]);
        }
        return Ok(Screen {
            text: format!(
                "{}\nPrefix: {}\nLanguage: {}",
                ctx.tr("config", &[]),
                ctx.db.prefix(),
                ctx.db.language()
            ),
            buttons: rows.into_iter().take(30).collect(),
            ..Default::default()
        });
    }
    let (config, _, validators) = config_data(ctx, module)?;
    let map = config
        .as_object()
        .context("module has no object configuration")?;
    let mut text = format!("<b>{}</b>\n", escape(module));
    let mut rows = vec![];
    for (key, value) in map.iter().take(25) {
        let hidden = validators.get(key).is_some_and(|v| v.hidden())
            || key.to_lowercase().contains("token")
            || key.to_lowercase().contains("secret")
            || key.to_lowercase().contains("password");
        let shown = if hidden {
            "••••••".into()
        } else {
            serde_json::to_string(value)?
        };
        text.push_str(&format!(
            "\n<b>{}</b>: <code>{}</code>",
            escape(key),
            escape(&shown.chars().take(200).collect::<String>())
        ));
        let mut row = vec![];
        match value {
            Value::Bool(v) => row.push(button(
                format!("{key}: {v}"),
                Action::ConfigToggle {
                    module: module.into(),
                    key: key.clone(),
                },
            )),
            Value::Number(n) if n.is_i64() => {
                row.push(button(
                    format!("− {key}"),
                    Action::ConfigStep {
                        module: module.into(),
                        key: key.clone(),
                        step: -1,
                    },
                ));
                row.push(button(
                    format!("+ {key}"),
                    Action::ConfigStep {
                        module: module.into(),
                        key: key.clone(),
                        step: 1,
                    },
                ));
            }
            _ => {}
        }
        if let Some(crate::validators::Validator::Choice { values }) = validators.get(key) {
            for choice in values.iter().take(4) {
                row.push(button(
                    serde_json::to_string(choice)?
                        .chars()
                        .take(32)
                        .collect::<String>(),
                    Action::ConfigSet {
                        module: module.into(),
                        key: key.clone(),
                        value: choice.clone(),
                    },
                ));
            }
        }
        if row.len() < 6 {
            row.push(button(
                format!("✎ {key}"),
                Action::ConfigInput {
                    module: module.into(),
                    key: key.clone(),
                },
            ));
        }
        row.push(button(
            format!("↺ {key}"),
            Action::ConfigReset {
                module: module.into(),
                key: key.clone(),
            },
        ));
        rows.push(row);
    }
    text.push_str(&format!(
        "\n\n<code>{}fconfig {} key &lt;JSON&gt;</code>",
        escape(&ctx.db.prefix()),
        escape(module)
    ));
    if module == crate::info::MODULE {
        text.push_str(&format!(
            "\n\nPlaceholders: <code>{}</code>",
            crate::info::PLACEHOLDERS
                .iter()
                .map(|s| format!("{{{s}}}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    rows.push(vec![button(
        ctx.tr("back", &[]),
        Action::ConfigOpen {
            module: String::new(),
        },
    )]);
    Ok(Screen {
        text,
        html: true,
        buttons: rows,
        ..Default::default()
    })
}
pub fn set_config(ctx: &Context, module: &str, key: &str, value: Value) -> Result<()> {
    let (mut config, _, validators) = config_data(ctx, module)?;
    let value = if let Some(validator) = validators.get(key) {
        validator.validate(value)?
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
    ctx.db.set(module, "__config__", config)
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
    if module == crate::update_ui::MODULE {
        return Ok((
            serde_json::to_value(crate::update_ui::Config::load(&ctx.db)?)?,
            serde_json::to_value(crate::update_ui::Config::default())?,
            Default::default(),
        ));
    }
    if module == crate::info::MODULE {
        return Ok((
            serde_json::to_value(crate::info::Config::load(&ctx.db)?)?,
            serde_json::to_value(crate::info::Config::default())?,
            Default::default(),
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
    let saved = ctx.db.get(module, "__config__");
    Ok((
        if saved.is_null() {
            package.manifest.config.clone()
        } else {
            saved
        },
        package.manifest.config,
        package.manifest.validators,
    ))
}
fn with_help_banner(ctx: &Context, mut screen: Screen) -> Result<Screen> {
    let url = crate::info::Config::load(&ctx.db)?.help_banner_url;
    if !url.is_empty() {
        let text = if screen.html {
            screen.text.clone()
        } else {
            escape(&screen.text)
        };
        screen.rich_html = Some(format!(
            "<img src=\"{}\"/>\n{}",
            escape(&url),
            text.replace('\n', "<br>")
        ));
    }
    Ok(screen)
}
