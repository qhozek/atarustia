use atarustia::{
    forms::{Action, Key, Screen},
    module_effects::{Output, Plan},
    modules::{Effect, Package},
};
use serde_json::json;

fn media() -> atarustia::media::Media {
    atarustia::media::Media::from_url(
        atarustia::file_id::FileKind::Photo,
        "https://example.com/pic.jpg",
    )
}
#[test]
fn all_contexts_use_the_same_output_plan_without_silent_field_loss() {
    let effects = [
        Effect::default(),
        Effect {
            text: Some("hello".into()),
            ..Default::default()
        },
        Effect {
            screen: Some(Screen {
                text: "screen".into(),
                ..Default::default()
            }),
            ..Default::default()
        },
        Effect {
            media: Some(media()),
            text: Some("caption".into()),
            ..Default::default()
        },
        Effect {
            album: vec![media(), media()],
            text: Some("album".into()),
            ..Default::default()
        },
        Effect {
            frames: vec!["one".into(), "two".into()],
            frame_ms: 300,
            ..Default::default()
        },
    ];
    for effect in effects {
        let plan = Plan::new("demo", effect).unwrap();
        match &plan.output {
            Output::None | Output::Text(_) | Output::Screen(_) | Output::Media(..) => {
                plan.require_single_result().unwrap()
            }
            Output::Album(album, text) => {
                assert_eq!(album.len(), 2);
                assert_eq!(text, "album");
                assert!(plan.require_single_result().is_err());
                assert!(plan.require_callback_destination(false).is_err());
                plan.require_callback_destination(true).unwrap();
            }
            Output::Frames(frames, delay) => {
                assert_eq!(frames.len(), 2);
                assert_eq!(*delay, 300);
                assert!(plan.require_single_result().is_err());
            }
        }
    }
}
#[test]
fn effect_validation_precedes_state_commit_and_rejects_privileged_buttons() {
    let screen = Screen {
        text: "bad".into(),
        buttons: vec![vec![Key {
            text: "core".into(),
            url: None,
            action: Some(Action::ConfigOpen {
                module: "Updater".into(),
            }),
        }]],
        ..Default::default()
    };
    assert!(
        Plan::new(
            "demo",
            Effect {
                screen: Some(screen),
                state: Some(json!({"bad":true})),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        Plan::new(
            "demo",
            Effect {
                frames: vec!["🦀".repeat(2049)],
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        Plan::new(
            "demo",
            Effect {
                state: Some(json!("x".repeat(256 * 1024 + 1))),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        Plan::new(
            "demo",
            Effect {
                frames: vec!["a".into()],
                media: Some(media()),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        Plan::new(
            "demo",
            Effect {
                suspend: Some(String::new()),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn lifecycle_names_and_approval_hash_are_validated_without_execution() {
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    let original = p.revision();
    assert!(
        serde_json::to_value(&p).unwrap()["manifest"]
            .get("lifecycle")
            .is_none()
    );
    p.manifest.lifecycle.ready = Some("ready".into());
    p.source
        .push_str(" fn ready(e,c) { throw \"must not run on validation\"; }");
    p.validate().unwrap();
    assert_ne!(original, p.revision());
    p.manifest.lifecycle.ready = Some("not a function!".into());
    assert!(p.validate().is_err());
}
#[test]
fn corrupt_installed_module_is_preserved_and_healthy_modules_still_load() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = atarustia::modules::Registry::load(dir.path(), 1).unwrap();
    let p: Package = serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    let hash = registry.stage(p).unwrap();
    registry.approve(&hash).unwrap();
    let bad = dir.path().join("loaded_modules/1/broken.json");
    std::fs::write(&bad, b"{unrecoverable").unwrap();
    let registry = atarustia::modules::Registry::load(dir.path(), 1).unwrap();
    assert!(registry.command("hello").is_some());
    assert!(registry.load_errors().contains_key("broken"));
    assert_eq!(std::fs::read(&bad).unwrap(), b"{unrecoverable");
}
#[test]
fn scheduler_identity_includes_manifest_and_suspend_status() {
    let dir = tempfile::tempdir().unwrap();
    let db = atarustia::database::Database::open(dir.path(), 1).unwrap();
    let mut p: Package =
        serde_json::from_str(include_str!("../examples/hello.module.json")).unwrap();
    p.manifest.loops.push(atarustia::modules::Loop {
        function: "tick".into(),
        autostart: true,
        interval_seconds: 100,
        stop_clause: None,
        wait_before: false,
    });
    let mut scheduler = atarustia::scheduler::Scheduler::default();
    assert_eq!(scheduler.due(&[p.clone()], &db).len(), 1);
    assert!(scheduler.due(&[p.clone()], &db).is_empty());
    p.manifest.config = json!({"greeting":"changed"});
    assert_eq!(scheduler.due(&[p.clone()], &db).len(), 1);
    db.set(
        "RustModuleLifecycle",
        "hello",
        json!({"status":"suspended"}),
    )
    .unwrap();
    assert!(scheduler.due(&[p.clone()], &db).is_empty());
    db.set("RustModuleLifecycle", "hello", json!({"status":"active"}))
        .unwrap();
    assert_eq!(scheduler.due(&[p], &db).len(), 1);
}
