use atarustia::{
    commands::Context,
    config::Credentials,
    database::Database,
    modules::{Package, Registry},
    session::MasterKey,
    telegram::Connection,
};
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

struct Fixture {
    dir: tempfile::TempDir,
    connection: Connection,
    ctx: Arc<Context>,
}
#[tokio::test]
async fn help_catalogue_and_permissions_are_recomputed_after_revocation() {
    let f = Fixture::new();
    f.install(include_str!("../examples/counter.module.json"));
    let owner = atarustia::help::build(&f.ctx, 1, None, &Default::default())
        .await
        .unwrap();
    owner.validate().unwrap();
    assert!(owner.text.contains("counter"));
    let public = atarustia::help::build(
        &f.ctx,
        2,
        None,
        &atarustia::help::State {
            force: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!public.text.contains("counter"));
    f.ctx
        .db
        .set("atarustia.security", "masks", json!({"counter":"everyone"}))
        .unwrap();
    let allowed = atarustia::help::build(
        &f.ctx,
        2,
        None,
        &atarustia::help::State {
            module: Some("counter".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(allowed.text.contains(".counter"));
    f.ctx
        .db
        .set("atarustia.security", "masks", json!({"counter":"owner"}))
        .unwrap();
    let revoked = atarustia::help::build(
        &f.ctx,
        2,
        None,
        &atarustia::help::State {
            module: Some("counter".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!revoked.text.contains(".counter"));
    assert!(atarustia::help::toggle_hidden(&f.ctx, "ataraxiaInfo").unwrap());
    assert!(atarustia::preferences::hidden(&f.ctx.db, "ataraxiaInfo"));
    let (unit, _) = f
        .ctx
        .forms
        .lock()
        .unwrap()
        .create(owner, 1, vec![])
        .unwrap();
    let state = atarustia::help::State {
        module: Some("counter".into()),
        page: 2,
        ..Default::default()
    };
    f.ctx
        .forms
        .lock()
        .unwrap()
        .set_help_return(&unit, Some(state.clone()))
        .unwrap();
    let config = atarustia::screens::config_for_unit(&f.ctx, "counter", &unit).unwrap();
    assert!(config.buttons.iter().flatten().any(
        |k| matches!(&k.action,Some(atarustia::forms::Action::HelpNavigate{state:s}) if s==&state)
    ));
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn core_info_settings_share_ui_validation_and_persistence() {
    let f = Fixture::new();
    let module = atarustia::info::MODULE;
    atarustia::screens::config(&f.ctx, module)
        .unwrap()
        .validate()
        .unwrap();
    atarustia::screens::apply_config(&f.ctx, module, "rich_mode", None, false).unwrap();
    assert!(atarustia::info::Config::load(&f.ctx.db).unwrap().rich_mode);
    let before = f.ctx.db.get(module, "__config__");
    assert!(
        atarustia::screens::set_config(&f.ctx, module, "banner_url", json!("file:///secret"))
            .is_err()
    );
    assert!(
        atarustia::screens::set_config(&f.ctx, module, "custom_message", json!("{unknown}"))
            .is_err()
    );
    assert!(
        atarustia::screens::set_config(&f.ctx, module, "rich_mode", json!("not-a-bool")).is_err()
    );
    assert_eq!(before, f.ctx.db.get(module, "__config__"));
    atarustia::screens::set_config(
        &f.ctx,
        module,
        "custom_message",
        json!("<b>{me}</b> · {uptime}"),
    )
    .unwrap();
    atarustia::screens::apply_config(&f.ctx, module, "rich_mode", None, true).unwrap();
    let reopened = Database::open(f.dir.path(), 1).unwrap();
    assert!(!atarustia::info::Config::load(&reopened).unwrap().rich_mode);
    assert_eq!(
        atarustia::info::Config::load(&reopened)
            .unwrap()
            .custom_message
            .as_deref(),
        Some("<b>{me}</b> · {uptime}")
    );
    let help = atarustia::screens::help(&f.ctx, None).unwrap();
    help.validate().unwrap();
    assert!(help.rich_html.unwrap().contains("assets/ataraxia.png"));
    atarustia::screens::set_config(&f.ctx, module, "help_banner_url", json!("")).unwrap();
    assert!(
        !atarustia::screens::help(&f.ctx, None)
            .unwrap()
            .rich_html
            .unwrap()
            .contains("<img")
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn updater_notification_setting_uses_core_config_and_reset() {
    let f = Fixture::new();
    let module = atarustia::update_ui::MODULE;
    atarustia::screens::config(&f.ctx, module)
        .unwrap()
        .validate()
        .unwrap();
    atarustia::screens::apply_config(&f.ctx, module, "update_notifications", None, false).unwrap();
    assert!(
        !atarustia::update_ui::Config::load(&f.ctx.db)
            .unwrap()
            .update_notifications
    );
    let before = f.ctx.db.get(module, "__config__");
    assert!(
        atarustia::screens::set_config(&f.ctx, module, "update_notifications", json!("bad"))
            .is_err()
    );
    assert_eq!(before, f.ctx.db.get(module, "__config__"));
    atarustia::screens::apply_config(&f.ctx, module, "update_notifications", None, true).unwrap();
    assert!(
        atarustia::update_ui::Config::load(&Database::open(f.dir.path(), 1).unwrap())
            .unwrap()
            .update_notifications
    );
    f.connection.close().await.unwrap();
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let key = Arc::new(MasterKey::load(dir.path()).unwrap());
        let c = Credentials {
            api_id: 12345,
            api_hash: "0".repeat(32),
        };
        let connection = Connection::open(dir.path(), "test", &c, key).unwrap();
        let db = Arc::new(Database::open(dir.path(), 1).unwrap());
        let ctx = Arc::new(Context {
            client: connection.client.clone(),
            db,
            modules: Mutex::new(Registry::load(dir.path(), 1).unwrap()),
            owner: 1,
            username: Some("owner".into()),
            root: dir.path().into(),
            start: Instant::now(),
            allow_shell: false,
            stop: Default::default(),
            web: None,
            secrets: vec![],
            rate: Default::default(),
            semaphore: Arc::new(tokio::sync::Semaphore::new(32)),
            seen: Default::default(),
            logs: Default::default(),
            access: Default::default(),
            events: Default::default(),
            maintenance: Default::default(),
            account: "test".into(),
            forms: Default::default(),
            bot: Default::default(),
            jobs: Default::default(),
            api_limits: Default::default(),
            native: Default::default(),
        });
        Self {
            dir,
            connection,
            ctx,
        }
    }
    fn install(&self, source: &str) {
        let p: Package = serde_json::from_str(source).unwrap();
        let mut r = self.ctx.modules.lock().unwrap();
        let h = r.stage(p).unwrap();
        r.approve(&h).unwrap();
    }
}
fn lifecycle_package() -> Package {
    let mut package: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    package.manifest.lifecycle = atarustia::modules::Lifecycle {
        install: Some("install".into()),
        ready: Some("ready".into()),
        unload: Some("unload".into()),
        config_change: Some("changed".into()),
    };
    package.source = r#"
        fn events(saved) { if saved == () { [] } else { saved.events } }
        fn install(event, config) { let log = events(state); log.push("install"); #{state:#{events:log}} }
        fn ready(event, config) { let log = events(state); log.push("ready"); #{state:#{events:log}} }
        fn unload(event, config) { let log = events(state); log.push("unload:" + event.data.reason); #{state:#{events:log}} }
        fn changed(event, config) { let log = events(state); log.push("config:" + config.greeting); #{state:#{events:log,old:event.data.old,current:event.data["new"]}} }
        fn hello(args, config) { #{text:"ok"} }
    "#.into();
    package
}
#[tokio::test]
async fn package_lifecycle_install_ready_restart_replace_and_remove() {
    let f = Fixture::new();
    let package = lifecycle_package();
    let hash = f
        .ctx
        .modules
        .lock()
        .unwrap()
        .stage(package.clone())
        .unwrap();
    assert!(f.ctx.db.get("hello", "state").is_null());
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    assert_eq!(
        f.ctx.db.get("hello", "state")["events"],
        json!(["install", "ready"])
    );
    atarustia::module_lifecycle::startup(&f.ctx).await;
    assert_eq!(
        f.ctx.db.get("hello", "state")["events"],
        json!(["install", "ready", "ready"])
    );
    let mut next = package;
    next.source.push_str("\n// next revision");
    let hash = f.ctx.modules.lock().unwrap().stage(next).unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    assert_eq!(
        f.ctx.db.get("hello", "state")["events"],
        json!([
            "install",
            "ready",
            "ready",
            "unload:replace",
            "install",
            "ready"
        ])
    );
    atarustia::module_lifecycle::remove(&f.ctx, "hello")
        .await
        .unwrap();
    assert!(f.ctx.modules.lock().unwrap().command("hello").is_none());
    assert_eq!(
        f.ctx.db.get("hello", "state")["events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap(),
        "unload:remove"
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn pause_resume_and_self_suspend_cover_watchers_and_loops() {
    let f = Fixture::new();
    let mut package = lifecycle_package();
    package.manifest.watchers.push(atarustia::modules::Watcher {
        function: "watch".into(),
        ..Default::default()
    });
    package.manifest.loops.push(atarustia::modules::Loop {
        function: "tick".into(),
        autostart: true,
        interval_seconds: 1,
        wait_before: false,
        stop_clause: None,
    });
    package.source.push_str(r#"fn watch(e,c) { #{state:#{events:events(state),seen:true},suspend:"waiting for configuration"} } fn tick(e,c) { #{state:#{events:events(state),loop_ran:true}} }"#);
    let hash = f
        .ctx
        .modules
        .lock()
        .unwrap()
        .stage(package.clone())
        .unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    atarustia::module_lifecycle::suspend(&f.ctx, "hello")
        .await
        .unwrap();
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    let event = atarustia::modules::Event {
        kind: "message".into(),
        chat: 1,
        ..Default::default()
    };
    f.ctx.handle_event(event.clone()).await.unwrap();
    f.ctx
        .run_loop(package.clone(), package.manifest.loops[0].clone())
        .await
        .unwrap();
    assert!(f.ctx.db.get("hello", "state").get("seen").is_none());
    assert!(f.ctx.db.get("hello", "state").get("loop_ran").is_none());
    let mut scheduler = atarustia::scheduler::Scheduler::default();
    assert!(scheduler.due(&[package.clone()], &f.ctx.db).is_empty());
    atarustia::module_lifecycle::resume(&f.ctx, "hello")
        .await
        .unwrap();
    assert_eq!(scheduler.due(&[package], &f.ctx.db).len(), 1);
    f.ctx.handle_event(event).await.unwrap();
    assert_eq!(f.ctx.db.get("hello", "state")["seen"], true);
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    assert!(
        atarustia::module_lifecycle::status(&f.ctx)
            .to_string()
            .contains("waiting for configuration")
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn broken_ready_hook_suspends_only_its_package() {
    let f = Fixture::new();
    let mut bad = lifecycle_package();
    bad.source = "fn ready(e,c) { throw \"broken\"; } fn hello(a,c) { \"hi\" }".into();
    bad.manifest.lifecycle.install = None;
    let hash = f.ctx.modules.lock().unwrap().stage(bad).unwrap();
    assert!(
        atarustia::module_lifecycle::approve(&f.ctx, &hash)
            .await
            .is_err()
    );
    assert!(f.ctx.modules.lock().unwrap().package("hello").is_some());
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    assert_eq!(f.ctx.db.get("RustModuleErrors", "hello")["phase"], "ready");
    assert!(f.ctx.modules.lock().unwrap().pending(&hash).is_err());
    atarustia::module_lifecycle::remove(&f.ctx, "hello")
        .await
        .unwrap();
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn config_changes_are_coalesced_validated_and_dispatched_once() {
    let f = Fixture::new();
    let hash = f
        .ctx
        .modules
        .lock()
        .unwrap()
        .stage(lifecycle_package())
        .unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    atarustia::screens::set_config(&f.ctx, "hello", "greeting", json!("one")).unwrap();
    atarustia::screens::set_config(&f.ctx, "hello", "greeting", json!("two")).unwrap();
    assert!(atarustia::screens::set_config(&f.ctx, "hello", "missing", json!("bad")).is_err());
    atarustia::module_lifecycle::config_changes(&f.ctx).await;
    let state = f.ctx.db.get("hello", "state");
    assert_eq!(state["old"]["greeting"], "Hello");
    assert_eq!(state["current"]["greeting"], "two");
    assert_eq!(state["events"], json!(["install", "ready", "config:two"]));
    atarustia::module_lifecycle::config_changes(&f.ctx).await;
    assert_eq!(f.ctx.db.get("hello", "state"), state);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn unload_runs_on_shutdown_even_after_account_cancellation() {
    let f = Fixture::new();
    let hash = f
        .ctx
        .modules
        .lock()
        .unwrap()
        .stage(lifecycle_package())
        .unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    f.ctx.stop.cancel();
    atarustia::module_lifecycle::shutdown(&f.ctx).await;
    assert_eq!(
        f.ctx.db.get("hello", "state")["events"],
        json!(["install", "ready", "unload:shutdown"])
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn watcher_errors_do_not_skip_other_handlers_or_modules() {
    let f = Fixture::new();
    let mut first: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    first.source="fn hello(a,c){\"ok\"} fn broken(e,c) {throw \"bad watcher\";} fn good(e,c){#{state:#{seen:true}}}".into();
    first.manifest.watchers = vec![
        atarustia::modules::Watcher {
            function: "broken".into(),
            ..Default::default()
        },
        atarustia::modules::Watcher {
            function: "good".into(),
            ..Default::default()
        },
    ];
    f.install(&serde_json::to_string(&first).unwrap());
    let mut second = first.clone();
    second.manifest.name = "second".into();
    second.manifest.commands.clear();
    f.install(&serde_json::to_string(&second).unwrap());
    f.ctx
        .handle_event(atarustia::modules::Event {
            kind: "message".into(),
            chat: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(f.ctx.db.get("hello", "state")["seen"], true);
    assert_eq!(f.ctx.db.get("second", "state")["seen"], true);
    assert_eq!(
        f.ctx.db.get("RustModuleErrors", "hello")["phase"],
        "watcher"
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn preview_never_commits_state_or_self_suspension() {
    let f = Fixture::new();
    let plan = atarustia::module_effects::Plan::new(
        "hello",
        atarustia::modules::Effect {
            state: Some(json!({"must_not_persist":true})),
            suspend: Some("preview".into()),
            text: Some("preview".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        plan.commit(&f.ctx, "hello", true).unwrap(),
        atarustia::module_effects::Output::Text(_)
    ));
    assert!(f.ctx.db.get("hello", "state").is_null());
    assert!(atarustia::module_lifecycle::active(&f.ctx, "hello"));
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn suspended_module_invalidates_bound_forms_even_without_callbacks() {
    let f = Fixture::new();
    f.install(include_str!("../examples/hello.module.json"));
    let (unit, _) = f
        .ctx
        .forms
        .lock()
        .unwrap()
        .create(
            atarustia::forms::Screen {
                text: "pages".into(),
                pages: vec!["a".into(), "b".into()],
                ..Default::default()
            },
            1,
            vec![],
        )
        .unwrap();
    f.ctx
        .forms
        .lock()
        .unwrap()
        .bind_module(&unit, "hello")
        .unwrap();
    atarustia::module_lifecycle::suspend(&f.ctx, "hello")
        .await
        .unwrap();
    assert!(f.ctx.forms.lock().unwrap().render(&unit, 0).is_err());
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn stale_queued_loop_cannot_run_after_manifest_only_update() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.manifest.loops.push(atarustia::modules::Loop {
        function: "tick".into(),
        autostart: true,
        interval_seconds: 1,
        wait_before: false,
        stop_clause: None,
    });
    p.source
        .push_str(" fn tick(e,c) { #{state:#{stale:true}} }");
    f.install(&serde_json::to_string(&p).unwrap());
    let mut next = p.clone();
    next.manifest.config = json!({"greeting":"new"});
    let hash = f.ctx.modules.lock().unwrap().stage(next).unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    f.ctx
        .run_loop(p.clone(), p.manifest.loops[0].clone())
        .await
        .unwrap();
    assert!(f.ctx.db.get("hello", "state").is_null());
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn startup_gate_and_persisted_owner_pause_survive_database_reopen() {
    let f = Fixture::new();
    f.install(include_str!("../examples/hello.module.json"));
    atarustia::module_lifecycle::prepare_startup(&f.ctx).unwrap();
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    atarustia::module_lifecycle::startup(&f.ctx).await;
    assert!(atarustia::module_lifecycle::active(&f.ctx, "hello"));
    atarustia::module_lifecycle::suspend(&f.ctx, "hello")
        .await
        .unwrap();
    atarustia::module_lifecycle::prepare_startup(&f.ctx).unwrap();
    atarustia::module_lifecycle::startup(&f.ctx).await;
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    assert_eq!(
        Database::open(f.dir.path(), 1)
            .unwrap()
            .get("RustModuleLifecycle", "hello")["status"],
        "suspended"
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn inline_results_preserve_screen_buttons_rich_media_and_invalidation() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/lifecycle.module.json")).unwrap();
    p.validate().unwrap();
    p.manifest.name = "hello".into();
    let key = atarustia::forms::Key {
        text: "refresh".into(),
        url: None,
        action: Some(atarustia::forms::Action::Module {
            module: "hello".into(),
            function: "refresh".into(),
            args: json!({}),
        }),
    };
    let screen = atarustia::forms::Screen {
        text: "fallback".into(),
        rich_html: Some("<b>Rich</b> {{atarustia:tap:0}}".into()),
        rich_buttons: vec![key],
        ..Default::default()
    };
    let output = atarustia::module_effects::Plan::new(
        "hello",
        atarustia::modules::Effect {
            screen: Some(screen),
            ..Default::default()
        },
    )
    .unwrap()
    .commit(&f.ctx, "hello", true)
    .unwrap();
    let result = atarustia::module_effects::inline(&f.ctx, "hello", "hello", output, 1).unwrap();
    let grammers_client::tl::enums::InputBotInlineResult::Result(result) = result else {
        panic!("not an article")
    };
    let grammers_client::tl::enums::InputBotInlineMessage::RichMessage(message) =
        result.send_message
    else {
        panic!("rich source lost")
    };
    let grammers_client::tl::enums::InputRichMessage::Html(html) = message.rich_message else {
        panic!("rich HTML lost")
    };
    assert!(html.html.contains("Rich"));
    assert!(html.html.contains("f:"));
    f.ctx.forms.lock().unwrap().invalidate_module("hello");
    // The manifest/library validation path must still never execute lifecycle code.
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn lifecycle_example_runs_install_ready_config_suspend_and_resume() {
    let f = Fixture::new();
    let p: Package =
        serde_json::from_str(include_str!("../examples/lifecycle.module.json")).unwrap();
    let hash = f.ctx.modules.lock().unwrap().stage(p).unwrap();
    atarustia::module_lifecycle::approve(&f.ctx, &hash)
        .await
        .unwrap();
    assert_eq!(f.ctx.db.get("lifecycle_demo", "state")["starts"], 1);
    atarustia::screens::set_config(&f.ctx, "lifecycle_demo", "label", json!("changed")).unwrap();
    atarustia::module_lifecycle::config_changes(&f.ctx).await;
    assert_eq!(f.ctx.db.get("lifecycle_demo", "state")["label"], "changed");
    atarustia::module_lifecycle::suspend(&f.ctx, "lifecycle_demo")
        .await
        .unwrap();
    atarustia::module_lifecycle::resume(&f.ctx, "lifecycle_demo")
        .await
        .unwrap();
    assert_eq!(f.ctx.db.get("lifecycle_demo", "state")["starts"], 2);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn ready_error_at_startup_does_not_abort_other_packages() {
    let f = Fixture::new();
    let mut broken = lifecycle_package();
    broken.manifest.lifecycle.install = None;
    broken.source = "fn hello(a,c){\"ok\"} fn ready(e,c){throw \"ready failed\";}".into();
    f.install(&serde_json::to_string(&broken).unwrap());
    let mut good = lifecycle_package();
    good.manifest.name = "healthy".into();
    good.manifest.commands[0].name = "healthy".into();
    good.manifest.commands[0].function = "hello".into();
    f.install(&serde_json::to_string(&good).unwrap());
    atarustia::module_lifecycle::prepare_startup(&f.ctx).unwrap();
    atarustia::module_lifecycle::startup(&f.ctx).await;
    assert!(!atarustia::module_lifecycle::active(&f.ctx, "hello"));
    assert!(atarustia::module_lifecycle::active(&f.ctx, "healthy"));
    assert_eq!(f.ctx.db.get("healthy", "state")["events"], json!(["ready"]));
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn unload_error_never_prevents_removal_or_leaves_callbacks() {
    let f = Fixture::new();
    let mut broken = lifecycle_package();
    broken.source = "fn hello(a,c){\"ok\"} fn unload(e,c){throw \"unload failed\";}".into();
    f.install(&serde_json::to_string(&broken).unwrap());
    atarustia::module_lifecycle::remove(&f.ctx, "hello")
        .await
        .unwrap();
    assert!(f.ctx.modules.lock().unwrap().package("hello").is_none());
    assert_eq!(f.ctx.db.get("RustModuleErrors", "hello")["phase"], "unload");
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn failed_config_hook_keeps_valid_config_and_other_hooks_continue() {
    let f = Fixture::new();
    let mut broken = lifecycle_package();
    broken.source = "fn hello(a,c){\"ok\"} fn changed(e,c){throw \"config failed\";}".into();
    f.install(&serde_json::to_string(&broken).unwrap());
    atarustia::screens::set_config(&f.ctx, "hello", "greeting", json!("saved")).unwrap();
    atarustia::module_lifecycle::config_changes(&f.ctx).await;
    assert_eq!(f.ctx.db.get("hello", "__config__")["greeting"], "saved");
    assert_eq!(
        f.ctx.db.get("RustModuleErrors", "hello")["phase"],
        "config_change"
    );
    assert!(f.ctx.db.get("RustModuleConfigChanges", "hello").is_null());
    assert!(atarustia::module_lifecycle::active(&f.ctx, "hello"));
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn invalid_watcher_effect_never_commits_state_and_next_handler_still_runs() {
    let f = Fixture::new();
    let mut package: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    package.manifest.watchers = vec![
        atarustia::modules::Watcher {
            function: "invalid".into(),
            ..Default::default()
        },
        atarustia::modules::Watcher {
            function: "after".into(),
            ..Default::default()
        },
    ];
    package.source=r#"fn hello(a,c){"hi"} fn invalid(e,c){#{state:#{bad:true},screen:#{text:"bad",buttons:[[#{text:"bad",action:#{"type":"config_open","module":"Updater"}}]]}}} fn after(e,c){#{state:#{previous:state,good:true}}}"#.into();
    f.install(&serde_json::to_string(&package).unwrap());
    f.ctx
        .handle_event(atarustia::modules::Event {
            kind: "message".into(),
            chat: 1,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(f.ctx.db.get("hello", "state")["previous"].is_null());
    assert_eq!(f.ctx.db.get("hello", "state")["good"], true);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn installed_event_handlers_receive_host_context_without_implicit_io() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.manifest.watchers.push(atarustia::modules::Watcher {
        function: "watch".into(),
        ..Default::default()
    });
    p.source.push_str(
        r#"
        fn watch(event, config) {
            let invocation_context = invocation();
            let denied = false;
            try { http_get("https://not-granted.example/"); } catch(error) { denied = true; }
            #{state: #{chat:invocation_context.chat, message:invocation_context.message_id, denied:denied}}
        }
    "#,
    );
    f.install(&serde_json::to_string(&p).unwrap());
    f.ctx
        .handle_event(atarustia::modules::Event {
            chat: -123,
            id: 7,
            kind: "message".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        f.ctx.db.get("hello", "state"),
        json!({"chat":-123,"message":7,"denied":true})
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn stateful_command_and_callback_round_trip() {
    let f = Fixture::new();
    f.install(include_str!("../examples/counter.module.json"));
    let (p, _) = f.ctx.modules.lock().unwrap().command("counter").unwrap();
    let first = p
        .run_command("counter", "", p.manifest.config.clone(), json!(null), "en")
        .unwrap();
    assert_eq!(first.state.as_ref().unwrap()["count"], 0);
    let screen = first.screen.unwrap();
    screen.validate().unwrap();
    assert!(screen.text.contains('0'));
    let next = p
        .run_event(
            "increment",
            json!({"kind":"callback"}),
            p.manifest.config.clone(),
            first.state.unwrap(),
            "en",
        )
        .unwrap();
    assert_eq!(next.state.unwrap()["count"], 1);
    assert!(next.screen.unwrap().text.contains('1'));
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn module_can_return_direct_media_without_network() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.source="fn hello(args, config) { #{text:\"Photo\",media:#{kind:\"photo\",source:#{\"type\":\"url\",url:\"https://example.com/photo.jpg\"}}} }".into();
    p.validate().unwrap();
    let effect = p
        .run_command(
            "hello",
            "",
            serde_json::json!({}),
            serde_json::Value::Null,
            "en",
        )
        .unwrap();
    assert_eq!(
        effect.media.unwrap().kind,
        atarustia::file_id::FileKind::Photo
    );
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn interactive_settings_save_and_reset_persist() {
    let f = Fixture::new();
    f.install(include_str!("../examples/counter.module.json"));
    atarustia::screens::apply_config(&f.ctx, "counter", "enabled", None, false).unwrap();
    assert_eq!(f.ctx.db.get("counter", "__config__")["enabled"], false);
    atarustia::screens::apply_config(&f.ctx, "counter", "step", Some(2), false).unwrap();
    assert_eq!(f.ctx.db.get("counter", "__config__")["step"], 3);
    atarustia::screens::apply_config(&f.ctx, "counter", "step", None, true).unwrap();
    assert_eq!(
        Database::open(f.dir.path(), 1)
            .unwrap()
            .get("counter", "__config__")["step"],
        1
    );
    atarustia::screens::config(&f.ctx, "counter")
        .unwrap()
        .validate()
        .unwrap();
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn config_validator_rejection_is_atomic() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/counter.module.json")).unwrap();
    p.manifest.validators.insert(
        "step".into(),
        atarustia::validators::Validator::Integer {
            min: Some(1),
            max: Some(2),
        },
    );
    f.install(&serde_json::to_string(&p).unwrap());
    atarustia::screens::apply_config(&f.ctx, "counter", "step", Some(1), false).unwrap();
    assert!(atarustia::screens::apply_config(&f.ctx, "counter", "step", Some(1), false).is_err());
    assert_eq!(f.ctx.db.get("counter", "__config__")["step"], 2);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn typed_configuration_input_uses_the_same_validator() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/counter.module.json")).unwrap();
    p.manifest.validators.insert(
        "step".into(),
        atarustia::validators::Validator::Integer {
            min: Some(1),
            max: Some(10),
        },
    );
    f.install(&serde_json::to_string(&p).unwrap());
    atarustia::screens::set_config(&f.ctx, "counter", "step", json!("5")).unwrap();
    assert_eq!(f.ctx.db.get("counter", "__config__")["step"], 5);
    assert!(atarustia::screens::set_config(&f.ctx, "counter", "step", json!("-5")).is_err());
    assert!(atarustia::screens::set_config(&f.ctx, "counter", "unknown", json!(1)).is_err());
    assert_eq!(f.ctx.db.get("counter", "__config__")["step"], 5);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn security_group_rules_and_inline_gate() {
    let f = Fixture::new();
    let group = atarustia::security::SecurityGroup {
        name: "helpers".into(),
        users: vec![2],
        permissions: vec![
            atarustia::security::parse_rule("module/counter", 60).unwrap(),
            atarustia::security::parse_rule("inline/ping", 60).unwrap(),
        ],
    };
    f.ctx
        .db
        .set("atarustia.security", "sgroups", json!({"helpers":group}))
        .unwrap();
    assert!(atarustia::security::targeted(
        &f.ctx.db,
        2,
        -5,
        "counter",
        Some("counter"),
        false
    ));
    assert!(!atarustia::security::targeted(
        &f.ctx.db,
        3,
        -5,
        "counter",
        Some("counter"),
        false
    ));
    assert!(!atarustia::security_commands::inline_allowed(
        &f.ctx.db, 1, 2, "ping"
    ));
    f.ctx
        .db
        .set("atarustia.security", "allow_inline_query", true)
        .unwrap();
    assert!(atarustia::security_commands::inline_allowed(
        &f.ctx.db, 1, 2, "ping"
    ));
    assert!(!atarustia::security_commands::inline_allowed(
        &f.ctx.db, 1, 2, "help"
    ));
    assert!(atarustia::security::parse_rule("command/terminal", 60).is_err());
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn pinned_library_imports_never_resolve_files() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.manifest
        .libraries
        .insert("math".into(), "fn twice(x) { x * 2 }".into());
    p.source = "import \"math\" as m; fn hello(args, config) { m::twice(21) }".into();
    p.validate().unwrap();
    assert_eq!(p.run("hello", "", json!({})).unwrap(), "42");
    p.source = "import \"outside\" as x; fn hello(args, config) { x::secret() }".into();
    assert!(p.run("hello", "", json!({})).is_err());
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn native_sdk_lifecycle_is_driven_by_services() {
    use atarustia::sdk::{Module, ModuleFuture, Services};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Native(Arc<AtomicUsize>);
    impl Module for Native {
        fn name(&self) -> &str {
            "lifecycle"
        }
        fn description(&self) -> &str {
            "test"
        }
        fn ready<'a>(&'a self, services: &'a Services) -> ModuleFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                services.db.set("native", "ready", true)?;
                Ok(())
            })
        }
        fn unload<'a>(&'a self, _services: &'a Services) -> ModuleFuture<'a> {
            Box::pin(async move {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        }
    }
    let f = Fixture::new();
    let count = Arc::new(AtomicUsize::new(0));
    let mut registry = atarustia::sdk::Registry::default();
    registry.register(Arc::new(Native(count.clone()))).unwrap();
    registry.ready(&f.ctx.services()).await.unwrap();
    registry.unload(&f.ctx.services()).await;
    assert_eq!(count.load(Ordering::SeqCst), 2);
    assert_eq!(f.ctx.db.get("native", "ready"), true);
    f.connection.close().await.unwrap();
}
#[tokio::test]
async fn hidden_help_and_disabled_watchers_do_not_modify_state() {
    let f = Fixture::new();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.manifest.watchers.push(atarustia::modules::Watcher {
        function: "watch".into(),
        contains: Some("hello".into()),
        outgoing: Some(false),
        ..Default::default()
    });
    p.source
        .push_str(" fn watch(event, config) { #{state:#{seen:true}} } ");
    let text = serde_json::to_string(&p).unwrap();
    f.install(&text);
    let event = atarustia::modules::Event {
        text: "hello".into(),
        chat: -10,
        sender: Some(2),
        outgoing: false,
        command: false,
        edited: false,
        id: 1,
        kind: "message".into(),
        data: serde_json::Value::Null,
        ..Default::default()
    };
    f.ctx.handle_event(event.clone()).await.unwrap();
    assert_eq!(f.ctx.db.get("hello", "state")["seen"], true);
    f.ctx
        .db
        .set("hello", "state", json!({"seen":false}))
        .unwrap();
    f.ctx
        .db
        .set(
            "atarustia.main",
            "watcher_policies",
            json!({"hello":{"disabled":true}}),
        )
        .unwrap();
    f.ctx.handle_event(event).await.unwrap();
    assert_eq!(f.ctx.db.get("hello", "state")["seen"], false);
    f.ctx
        .db
        .set("atarustia.help", "hidden", json!(["hello"]))
        .unwrap();
    assert!(!f.ctx.help().contains("[hello]"));
    f.connection.close().await.unwrap();
}
