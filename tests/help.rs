use atarustia::{
    forms::{Action, Forms},
    help::{self, Catalog, Command, Filter, Metadata, Module, State, Ui},
    security::Permission,
};
use std::collections::HashMap;
fn module(name: &str, core: bool) -> Module {
    Module {
        name: name.into(),
        description: "Description <&> with a useful summary".into(),
        core,
        configurable: true,
        hidden: false,
        commands: vec![Command {
            name: format!("{name}_cmd"),
            description: "[arg] — Do <things>".into(),
            permission: Permission::Owner,
            inline: false,
            guest: false,
            text_allowed: true,
        }],
        metadata: Metadata::default(),
    }
}
fn render(catalog: &Catalog, state: &State, owner: bool) -> atarustia::forms::Screen {
    help::render(
        catalog,
        state,
        Ui {
            english: false,
            premium: false,
        },
        ".",
        "test_bot",
        owner,
        false,
        &HashMap::new(),
        "https://example.com/banner.png",
    )
    .unwrap()
}
fn catalog() -> Catalog {
    Catalog {
        modules: vec![module("Core", true)]
            .into_iter()
            .chain((0..31).map(|i| module(&format!("ext{i:02}"), false)))
            .collect(),
        libraries: vec!["utils".into()],
    }
}
#[test]
fn catalogue_has_original_sections_fifteen_external_entries_and_core_on_every_page() {
    let c = catalog();
    for page in 0..3 {
        let screen = render(
            &c,
            &State {
                page,
                ..Default::default()
            },
            true,
        );
        screen.validate().unwrap();
        assert!(screen.text.contains("Встроенные модули"));
        assert!(screen.text.contains("Core"));
        assert!(screen.rich_html.as_ref().unwrap().contains("<img"));
        assert!(screen.rich_html.as_ref().unwrap().contains("<details"));
        assert_eq!(screen.rich_buttons.iter().filter(|k|matches!(&k.action,Some(Action::HelpNavigate{state}) if state.module.is_some())).count(),if page<2{16}else{2});
        assert!(
            screen
                .buttons
                .iter()
                .flatten()
                .any(|k| k.text == format!("{} / 3", page + 1))
        );
    }
    assert!(!render(&c, &State::default(), true).text.contains("ext15"));
    assert!(
        render(
            &c,
            &State {
                page: 1,
                ..Default::default()
            },
            true
        )
        .text
        .contains("ext15")
    );
}
#[test]
fn flags_are_tokens_not_substrings_and_filter_core_external() {
    assert_eq!(
        State::parse("module-f -c").unwrap().module.as_deref(),
        Some("module-f")
    );
    let core = State::parse("-c").unwrap();
    assert!(core.force);
    assert_eq!(core.filter, Filter::Core);
    let c = catalog();
    assert!(!render(&c, &core, true).text.contains("ext00"));
    let external = State::parse("-l").unwrap();
    assert!(external.force);
    assert!(!render(&c, &external, true).text.contains("<b>Core</b>"));
    assert!(State::parse(&"x".repeat(1025)).is_err());
}
#[test]
fn resolution_by_command_alias_case_and_fuzzy_notice() {
    let c = catalog();
    let aliases = HashMap::from([("demo".into(), "ext00_cmd".into())]);
    assert_eq!(
        help::resolve(&c, ".demo", ".", &aliases).unwrap().0.name,
        "ext00"
    );
    assert!(help::resolve(&c, "cOrE", ".", &aliases).unwrap().1);
    let screen = render(
        &c,
        &State {
            module: Some("ext0".into()),
            ..Default::default()
        },
        true,
    );
    assert!(screen.text.contains("Точного совпадения нет"));
    assert!(
        render(
            &c,
            &State {
                module: Some("utils".into()),
                ..Default::default()
            },
            true
        )
        .text
        .contains("библиотекой")
    );
}
#[test]
fn card_has_usage_copy_inline_guest_metadata_and_owner_controls() {
    let mut m = module("demo", false);
    m.commands[0].inline = true;
    m.commands[0].guest = true;
    m.metadata = Metadata {
        version: Some("1.2".into()),
        developer: Some("Alice <&>".into()),
        banner_url: Some("https://example.com/module.png".into()),
    };
    let c = Catalog {
        modules: vec![m],
        libraries: vec![],
    };
    let state = State {
        module: Some("demo".into()),
        ..Default::default()
    };
    let screen = render(&c, &state, true);
    screen.validate().unwrap();
    let rich = screen.rich_html.as_ref().unwrap();
    assert!(rich.contains("v1.2"));
    assert!(rich.contains("module.png"));
    assert!(screen.text.contains("<i>[arg]</i>"));
    assert!(screen.text.contains("Do &lt;things&gt;"));
    assert!(screen.text.contains("@test_bot demo_cmd"));
    assert!(screen.text.contains("Гостевые"));
    assert!(screen.text.contains("Alice &lt;&amp;&gt;"));
    assert!(
        screen
            .rich_buttons
            .iter()
            .any(|k| matches!(&k.action,Some(Action::Copy{text}) if text==".demo_cmd"))
    );
    assert!(
        screen
            .buttons
            .iter()
            .flatten()
            .any(|k| matches!(k.action, Some(Action::HelpSettings { .. })))
    );
    let public = render(&c, &state, false);
    assert!(!public.buttons.iter().flatten().any(|k| matches!(
        k.action,
        Some(Action::HelpToggle { .. } | Action::HelpSettings { .. })
    )));
    assert!(screen.restrict_module("demo").is_err());
}
#[test]
fn hidden_catalogue_and_command_paging_keep_navigation_state() {
    let mut c = catalog();
    c.modules[1].hidden = true;
    let home = render(&c, &State::default(), true);
    assert!(!home.text.contains("<b>ext00</b>"));
    assert!(home.text.contains("скрыто 1"));
    assert!(
        render(
            &c,
            &State {
                force: true,
                ..Default::default()
            },
            true
        )
        .text
        .contains("ext00")
    );
    let hidden = render(
        &c,
        &State {
            hidden: true,
            ..Default::default()
        },
        true,
    );
    hidden.validate().unwrap();
    assert!(hidden.text.contains("Скрытые модули"));
    assert!(
        hidden
            .buttons
            .iter()
            .flatten()
            .any(|k| matches!(k.action, Some(Action::HelpUnhideAll { .. })))
    );
    let mut m = module("many", false);
    m.commands = (0..22)
        .map(|i| Command {
            name: format!("cmd{i:02}"),
            description: "[input] — description".into(),
            permission: Permission::Owner,
            inline: false,
            guest: false,
            text_allowed: true,
        })
        .collect();
    let c = Catalog {
        modules: vec![m],
        libraries: vec![],
    };
    let state = State {
        module: Some("many".into()),
        command_page: 1,
        page: 2,
        filter: Filter::External,
        ..Default::default()
    };
    let screen = render(&c, &state, true);
    assert!(screen.text.contains("cmd08"));
    assert!(!screen.text.contains("cmd00"));
    assert!(screen.buttons.iter().flatten().any(|k|matches!(&k.action,Some(Action::HelpNavigate{state}) if state.module.is_none()&&state.page==2&&state.filter==Filter::External)));
}
#[test]
fn rich_tokens_fallback_copy_and_permission_actions_remain_bound() {
    let c = catalog();
    let mut forms = Forms::default();
    let (id, _) = forms
        .create(render(&c, &State::default(), true), 1, vec![])
        .unwrap();
    let rich = forms.rich_source(&id).unwrap();
    assert!(!rich.contains("{{atarustia:"));
    assert!(rich.contains("f:"));
    forms.plain_fallback(&id);
    let input = forms.render(&id, 0).unwrap();
    let grammers_client::tl::enums::InputBotInlineMessage::Text(text) =
        grammers_client::update::Article::new("test", input)
            .raw
            .send_message
    else {
        panic!()
    };
    assert!(text.message.contains("Внешние модули"));
    assert!(!text.message.contains("<details>"));
    assert!(forms.rich_source(&id).is_none());
}
#[test]
fn metadata_rejects_unsafe_banners_and_usage_parser_is_literal() {
    for url in [
        "file:///secret",
        "http://example.com/a",
        "https://user:pass@example.com/a",
    ] {
        assert!(
            Metadata {
                banner_url: Some(url.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    assert_eq!(
        help::parse_doc("[a] <b> — description"),
        ("[a] <b>".into(), "description".into())
    );
    assert_eq!(
        help::parse_doc("normal description"),
        (String::new(), "normal description".into())
    );
}
#[test]
fn command_inline_and_guest_sections_are_all_reachable_by_pager() {
    let mut m = module("many", false);
    m.commands = (0..17)
        .map(|i| Command {
            name: format!("cmd{i:02}"),
            description: "description".into(),
            permission: Permission::Owner,
            inline: true,
            guest: true,
            text_allowed: true,
        })
        .collect();
    let c = Catalog {
        modules: vec![m],
        libraries: vec![],
    };
    let mut all = String::new();
    for page in 0..7 {
        let screen = render(
            &c,
            &State {
                module: Some("many".into()),
                command_page: page,
                ..Default::default()
            },
            true,
        );
        screen.validate().unwrap();
        all.push_str(&screen.text);
    }
    for i in 0..17 {
        assert!(all.contains(&format!(".cmd{i:02}")));
        assert_eq!(all.matches(&format!("@test_bot cmd{i:02}")).count(), 2);
    }
}
#[test]
fn large_description_and_hostile_metadata_remain_bounded_in_fallback() {
    let mut m = module("huge", false);
    m.description = "🦀<&>".repeat(2000);
    m.commands = (0..8)
        .map(|i| Command {
            name: format!("cmd{i}"),
            description: "🦀<&>".repeat(1000),
            permission: Permission::Owner,
            inline: false,
            guest: false,
            text_allowed: true,
        })
        .collect();
    let c = Catalog {
        modules: vec![m],
        libraries: vec![],
    };
    let screen = render(
        &c,
        &State {
            module: Some("huge".into()),
            ..Default::default()
        },
        true,
    );
    screen.validate().unwrap();
    assert!(screen.text.contains("&lt;&amp;&gt;"));
}
#[test]
fn private_help_navigation_context_is_not_serialized_into_module_payloads() {
    let screen = render(
        &catalog(),
        &State {
            page: 1,
            ..Default::default()
        },
        true,
    );
    assert!(screen.help_state.is_some());
    assert!(
        serde_json::to_value(screen)
            .unwrap()
            .get("help_state")
            .is_none()
    );
}
