//! Installed package lifecycle. All transitions share Context::events with handlers.
//! Hooks execute only after approval, never while parsing/staging/reviewing a package.
use crate::{commands::Context, modules::Package};
use anyhow::{Context as _, Result, ensure};
use serde_json::{Value, json};
const NS: &str = "RustModuleLifecycle";

pub fn active(context: &Context, module: &str) -> bool {
    context
        .db
        .get(NS, module)
        .get("status")
        .and_then(Value::as_str)
        .is_none_or(|s| s == "active")
}
/// Before the bot update loop is spawned, close the package gate. Only ready
/// hooks will reopen it; saved owner suspensions remain untouched.
pub fn prepare_startup(context: &Context) -> Result<()> {
    let packages = context.modules.lock().unwrap().export();
    for package in packages {
        if active(context, &package.manifest.name) {
            set_status(
                context,
                &package.manifest.name,
                "starting",
                "waiting for ready",
            )?;
        }
    }
    Ok(())
}
pub fn mark_suspended(context: &Context, module: &str, reason: &str) -> Result<()> {
    set_status(context, module, "suspended", reason)?;
    context.forms.lock().unwrap().invalidate_module(module);
    Ok(())
}
fn set_status(context: &Context, module: &str, status: &str, reason: &str) -> Result<()> {
    context.db.set(NS,module,json!({"status":status,"reason":context.logs.redact(reason.to_owned()).chars().take(1024).collect::<String>()}))
}
pub fn report(context: &Context, module: &str, phase: &str, error: &anyhow::Error) {
    let error = context.logs.redact(error.to_string());
    tracing::warn!(module,phase,error=%error,"module handler failed");
    let _ = context.db.set("RustModuleErrors",module,json!({"phase":phase,"error":error.chars().take(1024).collect::<String>(),"time":crate::security::now()}));
}
pub fn status(context: &Context) -> Value {
    let packages = context.modules.lock().unwrap().export();
    json!({"modules":packages.iter().map(|p| json!({"module":p.manifest.name,"lifecycle":context.db.get(NS,&p.manifest.name),"last_error":context.db.get("RustModuleErrors",&p.manifest.name)})).collect::<Vec<_>>(),"load_errors":context.modules.lock().unwrap().load_errors()})
}
async fn hook(context: &Context, package: &Package, phase: &str, data: Value) -> Result<()> {
    let invocation = hook_inner(context, package, phase, data);
    if phase == "unload" {
        tokio::time::timeout(std::time::Duration::from_secs(2), invocation)
            .await
            .context("unload hook timed out")?
    } else {
        invocation.await
    }
}
async fn hook_inner(context: &Context, package: &Package, phase: &str, data: Value) -> Result<()> {
    let function = match phase {
        "install" => &package.manifest.lifecycle.install,
        "ready" => &package.manifest.lifecycle.ready,
        "unload" => &package.manifest.lifecycle.unload,
        "config_change" => &package.manifest.lifecycle.config_change,
        _ => anyhow::bail!("unknown lifecycle hook"),
    };
    let Some(function) = function else {
        return Ok(());
    };
    let shutdown =
        phase == "unload" && data.get("reason").and_then(Value::as_str) == Some("shutdown");
    let stop = if shutdown {
        Default::default()
    } else {
        context.stop.clone()
    };
    let invocation = crate::module_host::run_event_with_stop(
        context,
        package.clone(),
        function.clone(),
        json!({"kind":phase,"data":data}),
        Default::default(),
        stop,
    );
    let effect = invocation.await?;
    let plan = crate::module_effects::Plan::new(&package.manifest.name, effect)?;
    let output = plan.commit(context, &package.manifest.name, false)?;
    crate::module_effects::background(
        context,
        &package.manifest.name,
        output,
        false,
        context.owner,
        None,
    )
    .await
}
/// Called under the events lock, after loading the account's saved packages.
pub async fn startup(context: &Context) {
    let _lock = context.events.lock().await;
    let packages = context.modules.lock().unwrap().export();
    for package in packages {
        let status = context.db.get(NS, &package.manifest.name);
        if !active(context, &package.manifest.name)
            && status.get("status").and_then(Value::as_str) != Some("starting")
        {
            continue;
        }
        let ready = async {
            let config = prepare_config(context, &package)?;
            context
                .db
                .set(&package.manifest.name, "__config__", config)?;
            set_status(
                context,
                &package.manifest.name,
                "initializing",
                "ready in progress",
            )?;
            hook(context, &package, "ready", Value::Null).await?;
            if context
                .db
                .get(NS, &package.manifest.name)
                .get("status")
                .and_then(Value::as_str)
                == Some("initializing")
            {
                set_status(context, &package.manifest.name, "active", "")?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = ready {
            report(context, &package.manifest.name, "ready", &error);
            let _ = mark_suspended(
                context,
                &package.manifest.name,
                "ready hook failed; see module status",
            );
        }
    }
}
pub async fn approve(context: &Context, hash: &str) -> Result<()> {
    let _lock = context.events.lock().await;
    let package = context.modules.lock().unwrap().pending(hash)?;
    let name = package.manifest.name.clone();
    let previous = context.modules.lock().unwrap().package(&name);
    let config = prepare_config(context, &package)?;
    // Durable approval precedes hook side effects. Failed hooks leave an installed,
    // suspended revision, which can be inspected/resumed rather than silently rolled back.
    set_status(context, &name, "initializing", "approval in progress")?;
    if let Err(error) = context.modules.lock().unwrap().approve(hash) {
        set_status(
            context,
            &name,
            "suspended",
            "approval failed; inspect installed revision",
        )?;
        return Err(error);
    }
    context.forms.lock().unwrap().invalidate_module(&name);
    if let Some(previous) = previous
        && let Err(error) = hook(context, &previous, "unload", json!({"reason":"replace"})).await
    {
        report(context, &name, "unload", &error);
    }
    context.forms.lock().unwrap().invalidate_module(&name);
    set_status(context, &name, "initializing", "install in progress")?;
    context.db.set(&name, "__config__", config)?;
    for phase in ["install", "ready"] {
        if let Err(error) = hook(context, &package, phase, Value::Null).await {
            report(context, &name, phase, &error);
            mark_suspended(context, &name, "lifecycle hook failed; see module status")?;
            return Err(error);
        }
        if context
            .db
            .get(NS, &name)
            .get("status")
            .and_then(Value::as_str)
            == Some("suspended")
        {
            break;
        }
    }
    if context
        .db
        .get(NS, &name)
        .get("status")
        .and_then(Value::as_str)
        == Some("initializing")
    {
        set_status(context, &name, "active", "")?;
    }
    Ok(())
}
pub async fn remove(context: &Context, name: &str) -> Result<()> {
    let _lock = context.events.lock().await;
    remove_locked(context, name).await
}
async fn remove_locked(context: &Context, name: &str) -> Result<()> {
    let package = context
        .modules
        .lock()
        .unwrap()
        .package(name)
        .context("module not installed")?;
    if let Err(error) = hook(context, &package, "unload", json!({"reason":"remove"})).await {
        report(context, name, "unload", &error);
    }
    context.modules.lock().unwrap().remove(name)?;
    context.forms.lock().unwrap().invalidate_module(name);
    context.db.set(NS, name, Value::Null)?;
    context
        .db
        .set("RustModuleConfigChanges", name, Value::Null)?;
    Ok(())
}
pub async fn clear(context: &Context) -> Result<()> {
    let _lock = context.events.lock().await;
    let packages = context.modules.lock().unwrap().export();
    for package in packages {
        remove_locked(context, &package.manifest.name).await?;
    }
    context.modules.lock().unwrap().clear()?;
    Ok(())
}
pub async fn suspend(context: &Context, name: &str) -> Result<()> {
    let _lock = context.events.lock().await;
    let package = context
        .modules
        .lock()
        .unwrap()
        .package(name)
        .context("module not installed")?;
    mark_suspended(context, name, "paused by owner")?;
    if let Err(error) = hook(context, &package, "unload", json!({"reason":"suspend"})).await {
        report(context, name, "unload", &error);
    }
    // unload may itself return control; owner-requested suspension always wins.
    mark_suspended(context, name, "paused by owner")
}
pub async fn resume(context: &Context, name: &str) -> Result<()> {
    let _lock = context.events.lock().await;
    let package = context
        .modules
        .lock()
        .unwrap()
        .package(name)
        .context("module not installed")?;
    ensure!(!active(context, name), "module is already active");
    let config = prepare_config(context, &package)?;
    context.db.set(name, "__config__", config)?;
    set_status(context, name, "initializing", "ready in progress")?;
    if let Err(error) = hook(context, &package, "ready", json!({"reason":"resume"})).await {
        report(context, name, "ready", &error);
        mark_suspended(context, name, "ready hook failed")?;
        return Err(error);
    }
    if context
        .db
        .get(NS, name)
        .get("status")
        .and_then(Value::as_str)
        == Some("initializing")
    {
        set_status(context, name, "active", "")?;
    }
    Ok(())
}
/// Synchronous config validation remains atomic; this queue coalesces changes
/// and is drained under the handler lock by live UI paths and the runtime tick.
pub async fn config_changes(context: &Context) {
    let _lock = context.events.lock().await;
    config_changes_locked(context).await;
}
pub async fn poll_config_changes(context: &Context) {
    if !context.modules.lock().unwrap().export().iter().any(|p| {
        !context
            .db
            .get("RustModuleConfigChanges", &p.manifest.name)
            .is_null()
    }) {
        return;
    }
    let Ok(_lock) = context.events.try_lock() else {
        return;
    };
    config_changes_locked(context).await;
}
async fn config_changes_locked(context: &Context) {
    let packages = context.modules.lock().unwrap().export();
    for package in packages {
        let name = &package.manifest.name;
        if !active(context, name) || context.db.get("RustModuleConfigChanges", name).is_null() {
            continue;
        }
        let data = context.db.transaction(|db| {
            Ok(db
                .get_mut("RustModuleConfigChanges")
                .and_then(Value::as_object_mut)
                .and_then(|map| map.remove(name))
                .unwrap_or(Value::Null))
        });
        let Ok(data) = data else {
            continue;
        };
        if let Err(error) = hook(context, &package, "config_change", data).await {
            report(context, name, "config_change", &error);
        }
    }
}
fn prepare_config(context: &Context, package: &Package) -> Result<Value> {
    let saved = context.db.get(&package.manifest.name, "__config__");
    let mut config = package.manifest.config.clone();
    if let (Some(defaults), Some(saved)) = (config.as_object_mut(), saved.as_object()) {
        for (key, value) in defaults.iter_mut() {
            if let Some(saved) = saved.get(key) {
                *value = saved.clone();
            }
        }
    } else if !saved.is_null() {
        config = saved;
    }
    for (key, validator) in &package.manifest.validators {
        let value = config.get(key).context("saved config key missing")?;
        let value = validator.validate(value.clone())?;
        config
            .as_object_mut()
            .context("invalid saved configuration")?
            .insert(key.clone(), value);
    }
    Ok(config)
}
pub async fn shutdown(context: &Context) {
    let _lock = context.events.lock().await;
    let packages = context.modules.lock().unwrap().export();
    for package in packages.into_iter().rev() {
        if !active(context, &package.manifest.name) {
            continue;
        }
        if let Err(error) = hook(context, &package, "unload", json!({"reason":"shutdown"})).await {
            report(context, &package.manifest.name, "unload", &error);
        }
    }
}
