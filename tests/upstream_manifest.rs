//! Structural checks only. These do not certify the pending behavioral/visual port.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

fn manifest() -> Value {
    serde_json::from_str(include_str!("../contracts/upstream-manifest.json")).unwrap()
}

#[test]
fn manifest_pin_policy_and_schema_are_explicit() {
    let m = manifest();
    assert_eq!(m["schema_version"], 1);
    assert_eq!(m["extractor_version"], 1);
    assert_eq!(m["upstream"]["revision"], atarustia::original::REVISION);
    assert_eq!(
        m["upstream"]["repository"],
        "https://git.break.wtf/xenside/ataraxia.git"
    );
    assert_eq!(
        m["extraction"]["arguments_reviewed_revision"],
        m["upstream"]["revision"]
    );
    let policy: Value =
        serde_json::from_str(include_str!("../contracts/port-policy.json")).unwrap();
    assert_eq!(m["policy"], policy);
    assert!(
        policy["unresolved_decisions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let decisions = policy["accepted_decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 2);
    for id in ["python-runtime", "file-compatibility"] {
        let decision = decisions.iter().find(|d| d["id"] == id).unwrap();
        assert_eq!(decision["status"], "accepted");
        assert_ne!(decision["implementation_status"], "complete");
        assert!(!decision["requirements"].as_array().unwrap().is_empty());
        let path = decision["design"].as_str().unwrap();
        assert!(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(path)
                .is_file()
        );
    }
    assert_eq!(decisions[0]["implementation_status"], "not_implemented");
    assert_eq!(decisions[1]["implementation_status"], "partial_import_only");
    assert!(
        !policy["known_deviations_not_accepted_as_parity"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        m["extraction"]["missing_argument_contracts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        m["extraction"]["obsolete_argument_contracts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn command_names_ownership_aliases_and_default_security_match_inventory() {
    let m = manifest();
    let commands = m["commands"].as_object().unwrap();
    let expected: BTreeSet<_> = commands.keys().map(String::as_str).collect();
    let actual: BTreeSet<_> = atarustia::commands::BUILTINS
        .iter()
        .chain(atarustia::preferences::COMMANDS)
        .chain(atarustia::security_commands::COMMANDS)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(commands.len(), 66);
    assert_eq!(m["modules"].as_object().unwrap().len(), 13);
    let empty = Default::default();
    for (name, command) in commands {
        assert_eq!(
            atarustia::help::core_group(name),
            command["module"].as_str().unwrap(),
            "{name}"
        );
        assert_eq!(command["security"]["mask"], 1, "{name}");
        for alias in command["aliases"].as_array().unwrap() {
            assert_eq!(
                atarustia::dispatcher::resolve(alias.as_str().unwrap(), "", &empty)
                    .unwrap()
                    .0,
                *name
            );
        }
        for field in ["syntax", "scenarios", "effects"] {
            assert!(
                !command["argument_contract"][field].is_null(),
                "{name}: {field}"
            );
        }
    }
}

#[test]
fn source_strings_and_copied_assets_are_verified_without_network() {
    let m = manifest();
    for (language, bytes) in [
        (
            "en",
            include_bytes!("../locales/original/en.yml").as_slice(),
        ),
        (
            "ru",
            include_bytes!("../locales/original/ru.yml").as_slice(),
        ),
    ] {
        let parsed: Value = serde_yaml::from_slice(bytes).unwrap();
        assert_eq!(
            parsed, m["strings"][language],
            "all keys and values, including quoted on/off"
        );
        let path = format!("ataraxia/langpacks/{language}.yml");
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            m["files"][&path]["sha256"]
        );
    }
    for (path, bytes) in [
        (
            "assets/ataraxia.png",
            include_bytes!("../assets/ataraxia.png").as_slice(),
        ),
        (
            "assets/ataraxia_backup.png",
            include_bytes!("../assets/ataraxia_backup.png").as_slice(),
        ),
        (
            "assets/bot_pfp.png",
            include_bytes!("../assets/bot_pfp.png").as_slice(),
        ),
        (
            "assets/banner.txt",
            include_bytes!("../assets/banner.txt").as_slice(),
        ),
        (
            "assets/success.txt",
            include_bytes!("../assets/success.txt").as_slice(),
        ),
        (
            "assets/2fa.txt",
            include_bytes!("../assets/2fa.txt").as_slice(),
        ),
    ] {
        assert_eq!(
            hex::encode(Sha256::digest(bytes)),
            m["assets"][path]["sha256"],
            "{path}"
        );
    }
}

#[test]
fn incomplete_surfaces_are_recorded_not_silently_marked_implemented() {
    let m = manifest();
    assert_eq!(m["configs"].as_object().unwrap().len(), 24);
    assert_eq!(m["configs"]["Executor.FLOOD_WAIT_PROTECT"]["default"], 2);
    assert_eq!(m["configs"]["Tester.tglog_level"]["default"], "ERROR");
    assert_eq!(
        m["configs"]["ataraxiaInfo.banner_url"]["default"],
        atarustia::info::BANNER.replace("/media/", "/raw/")
    );
    let methods = &m["bot_facade"]["InlineBot"]["methods"];
    for name in [
        "send_widget",
        "edit_message_widget_data",
        "edit_widget_data",
        "answer_widget_query",
    ] {
        assert!(methods[name]["signature"]["async"].as_bool().unwrap());
    }
    assert_eq!(
        m["events"]["raw_updates"]["UpdateMessageWidgetData"],
        "widget_data"
    );
    assert_eq!(m["screens"]["constants"]["KEYBOARD_LIMIT"], 100);
    // The declaration is a reference requirement, not an assertion that Rust has closed it.
    assert!(
        m["policy"]["acceptance"]["manifest_check_does_not_prove"]
            .as_str()
            .unwrap()
            .contains("Behavioral")
    );
}
