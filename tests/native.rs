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
    assert!(atarustia::help::toggle_hidden(&f.ctx, "atarustiaInfo").unwrap());
    assert!(atarustia::preferences::hidden(&f.ctx.db, "atarustiaInfo"));
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
    assert!(help.rich_html.unwrap().contains("assets/atarustia.png"));
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
