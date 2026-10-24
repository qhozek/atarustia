use atarustia::{
    config_ui::{self, Input, Module, Operation, State, Ui},
    forms::{Action, Forms},
    validators::Validator,
};
use serde_json::json;
use std::collections::BTreeMap;

fn module() -> Module {
    Module {
        name: "Example".into(),
        kind: "external",
        values: json!({"enabled":true,"count":0,"ratio":0.5,"choice":"a","choices":["a"],"items":["one","two"],"secret":"sensitive-value","long":"x".repeat(80)}),
        defaults: json!({"enabled":false,"count":0,"ratio":0.5,"choice":"a","choices":["a"],"items":["one","two"],"secret":"sensitive-value","long":"x".repeat(80)}),
        validators: BTreeMap::from([
            ("enabled".into(), Validator::Boolean),
            (
                "count".into(),
                Validator::Integer {
                    min: Some(0),
                    max: Some(5),
                },
            ),
            (
                "ratio".into(),
                Validator::Float {
                    min: Some(0.),
                    max: Some(1.),
                },
            ),
            (
                "choice".into(),
                Validator::Choice {
                    values: vec![json!("a"), json!("b")],
                },
            ),
            (
                "choices".into(),
                Validator::MultiChoice {
                    values: vec![json!("a"), json!("b")],
                },
            ),
            (
                "items".into(),
                Validator::Series {
                    item: Box::new(Validator::String {
                        min_len: None,
                        max_len: None,
                    }),
                    min_len: None,
                    max_len: Some(40),
                },
            ),
            (
                "secret".into(),
                Validator::Hidden {
                    inner: Box::new(Validator::String {
                        min_len: None,
                        max_len: None,
                    }),
                },
            ),
            (
                "long".into(),
                Validator::String {
                    min_len: None,
                    max_len: None,
                },
            ),
        ]),
        docs: BTreeMap::from([("count".into(), "Number <&>".into())]),
        folders: BTreeMap::from([("count".into(), "Shared".into())]),
    }
}
fn ui() -> Ui {
    Ui {
        english: false,
        premium: false,
    }
}
fn state(p: &str) -> State {
    State {
        module: Some("Example".into()),
        parameter: Some(p.into()),
        ..Default::default()
    }
}
fn render(state: &State) -> atarustia::forms::Screen {
    let s = config_ui::render(&[module()], state, ui()).unwrap();
    s.validate().unwrap();
    s
}
#[test]
fn original_home_groups_changes_folders_and_no_rebranding() {
    let s = render(&State::default());
    let r = s.rich_html.unwrap();
    assert!(r.contains("Настройки"));
    assert!(r.contains("Внешние"));
    assert!(r.contains("Папки"));
    assert!(r.contains("изменено 1"));
    assert!(!r.contains("<b>atarustia"));
    let s = render(&State {
        folder: Some("Shared".into()),
        ..Default::default()
    });
    assert!(s.text.contains("count"));
    assert!(!s.text.contains("enabled"));
    let s = render(&State {
        module: Some("Example".into()),
        folder: Some("Shared".into()),
        ..Default::default()
    });
    assert!(s.buttons.iter().flatten().any(|k|matches!(&k.action,Some(Action::Configure{state,..}) if state.folder.as_deref()==Some("Shared") && state.module.is_none())));
}
#[test]
fn parameter_boolean_rich_buttons_and_fallback_rows() {
    let s = render(&state("enabled"));
    assert!(s.rich_html.as_ref().unwrap().contains("<tg-button-row"));
    assert!(s.text.contains("Сейчас"));
    assert!(s.text.contains("По умолчанию"));
    assert!(!s.buttons.iter().flatten().any(|k| matches!(
        k.action,
        Some(Action::Configure {
            operation: Operation::Input { .. },
            ..
        })
    )));
    let rows = s.config_fallback.as_ref().unwrap();
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[1].len(), 2);
    let mut f = Forms::default();
    let (id, _) = f.create(s, 1, vec![]).unwrap();
    assert!(f.plain_fallback(&id));
    f.render(&id, 0).unwrap();
}
#[test]
fn bounded_numeric_steps_float_and_stale_mutations() {
    let s = render(&state("count"));
    assert!(s.text.contains("Number &lt;&amp;&gt;"));
    assert!(s.rich_html.as_ref().unwrap().contains("0…5"));
    let steps: Vec<_> = s
        .rich_buttons
        .iter()
        .filter_map(|k| {
            if let Some(Action::Configure {
                operation: Operation::Step { step, .. },
                ..
            }) = &k.action
            {
                Some(*step)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(steps, vec![1.]);
    assert!(
        config_ui::mutate(
            &module(),
            &state("count"),
            &Operation::Step {
                step: -1.,
                integer: true
            },
            None
        )
        .is_err()
    );
    let (_, v) = config_ui::mutate(
        &module(),
        &state("ratio"),
        &Operation::Step {
            step: 0.1,
            integer: false,
        },
        None,
    )
    .unwrap();
    assert_eq!(v, json!(0.6));
}
#[test]
fn choices_preserve_scalar_and_multi_choice_semantics() {
    let s = render(&state("choice"));
    assert!(s.rich_buttons.iter().any(|k| k.text == "● a"));
    assert!(
        !s.buttons
            .iter()
            .flatten()
            .any(|k| matches!(k.action, Some(Action::Copy { .. })))
    );
    let (_, v) = config_ui::mutate(
        &module(),
        &state("choices"),
        &Operation::Toggle { value: json!("b") },
        None,
    )
    .unwrap();
    assert_eq!(v, json!(["a", "b"]));
    let (_, v) = config_ui::mutate(
        &module(),
        &state("choices"),
        &Operation::Toggle { value: json!("a") },
        None,
    )
    .unwrap();
    assert_eq!(v, json!([]));
}
#[test]
fn hidden_values_never_leak_via_copy_or_default() {
    let s = render(&state("secret"));
    assert!(!s.text.contains("sensitive-value"));
    assert!(!s.rich_html.unwrap().contains("sensitive-value"));
    assert!(
        !s.buttons
            .iter()
            .flatten()
            .any(|k| matches!(k.action, Some(Action::Copy { .. })))
    );
    let shown = render(&State {
        show: true,
        ..state("secret")
    });
    assert!(shown.text.contains("sensitive-value"));
    assert!(shown.rich_buttons.iter().any(|k| k.text == "Скрыть"));
}
#[test]
fn long_value_reply_and_series_add_remove() {
    let s = render(&state("long"));
    assert!(s.text.contains("длинное значение — ответом на сообщение"));
    assert!(s.config_reply.is_some());
    let s = render(&state("items"));
    assert!(s.rich_buttons.iter().any(|k| matches!(
        k.action,
        Some(Action::Configure {
            operation: Operation::RemoveItem { index: 0, .. },
            ..
        })
    )));
    let (_, v) = config_ui::mutate(
        &module(),
        &state("items"),
        &Operation::Input { mode: Input::Add },
        Some("['three', 'four']"),
    )
    .unwrap();
    assert_eq!(v, json!(["one", "two", "three", "four"]));
    let (_, v) = config_ui::mutate(
        &module(),
        &state("items"),
        &Operation::RemoveItem {
            index: 19,
            item: json!("two"),
        },
        None,
    )
    .unwrap();
    assert_eq!(v, json!(["one"]));
    assert!(
        config_ui::mutate(
            &module(),
            &state("items"),
            &Operation::Input {
                mode: Input::Remove
            },
            Some("missing")
        )
        .is_err()
    );
}
#[test]
fn reset_all_is_a_separate_danger_confirmation() {
    let home = render(&State {
        module: Some("Example".into()),
        ..Default::default()
    });
    assert!(
        home.buttons
            .iter()
            .flatten()
            .any(|k| matches!(&k.action,Some(Action::Configure{state,..}) if state.confirm_reset))
    );
    let s = render(&State {
        module: Some("Example".into()),
        confirm_reset: true,
        ..Default::default()
    });
    assert!(s.text.contains("Сбросить все параметры"));
    assert!(matches!(
        s.button_appearance[0][0].style,
        atarustia::markup::Style::Danger
    ));
}
#[test]
fn input_tokens_are_scoped_expire_with_form_and_do_not_expose_secrets() {
    let s = render(&state("secret"));
    let mut forms = Forms::default();
    let (id, body) = forms.create(s, 1, vec![]).unwrap();
    let raw = grammers_client::update::Article::new("screen", body)
        .raw
        .send_message;
    let grammers_client::tl::enums::InputBotInlineMessage::Text(raw) = raw else {
        panic!("text")
    };
    let markup = raw.reply_markup.unwrap();
    let grammers_client::tl::enums::ReplyMarkup::ReplyInlineMarkup(markup) = markup else {
        panic!("markup")
    };
    let query = markup
        .rows
        .iter()
        .find_map(|row| {
            let grammers_client::tl::enums::KeyboardInlineButtonRow::Row(row) = row;
            row.buttons.iter().find_map(|b| {
                let grammers_client::tl::enums::KeyboardInlineButton::Button(b) = b;
                match &b.r#type {
                    grammers_client::tl::enums::InlineButtonType::SwitchInline(s) => {
                        Some(s.query.clone())
                    }
                    _ => None,
                }
            })
        })
        .unwrap();
    assert!(!query.contains("secret"));
    let (_, s, mode, value) = forms.config_input(&format!("{query}hello")).unwrap();
    assert_eq!(s.parameter.as_deref(), Some("secret"));
    assert_eq!(mode, Input::Value);
    assert_eq!(value, "hello");
    forms.remove(&id);
    assert!(forms.config_input(&query).is_none());
}
#[test]
fn settings_is_the_original_independent_menu() {
    let d = tempfile::tempdir().unwrap();
    let db = atarustia::database::Database::open(d.path(), 1).unwrap();
    let s = config_ui::settings_screen(&db, false);
    s.validate().unwrap();
    assert_eq!(
        s.text,
        atarustia::original::text(false, "settings", "inline_settings")
    );
    assert_eq!(
        s.buttons.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![3, 1, 2, 1]
    );
    assert_eq!(
        s.buttons[0]
            .iter()
            .map(|k| k.text.as_str())
            .collect::<Vec<_>>(),
        vec!["🚫 NoNick", "🚫 Grep", "✅ InlineLogs"]
    );
    db.set("atarustia.main", "no_nickname", true).unwrap();
    assert_eq!(
        config_ui::settings_screen(&db, false).buttons[0][0].text,
        "✅ NoNick"
    );
}
#[test]
fn literals_and_quoted_arguments_are_not_code_execution() {
    assert_eq!(
        config_ui::literal("['a', False, 0, None]"),
        json!(["a", false, 0, null])
    );
    assert_eq!(config_ui::literal("(1, 2)"), json!([1, 2]));
    assert_eq!(config_ui::literal("'x\\ny'"), json!("x\ny"));
    assert_eq!(
        config_ui::literal("__import__('os')"),
        json!("__import__('os')")
    );
    assert_eq!(
        config_ui::arguments("\"Weather module\" city"),
        vec!["Weather module", "city"]
    );
    let aliases = Default::default();
    assert_eq!(
        atarustia::dispatcher::resolve("cfg", "Updater", &aliases)
            .unwrap()
            .0,
        "config"
    );
    assert_eq!(
        atarustia::dispatcher::resolve("fcfg", "Updater x True", &aliases)
            .unwrap()
            .0,
        "fconfig"
    );
}

#[test]
fn rich_blocks_preformatted_values_and_fallback_html_remain_valid() {
    let s = render(&state("long"));
    assert_eq!(
        s.text.matches("<blockquote").count(),
        s.text.matches("</blockquote>").count()
    );
    assert!(!s.rich_html.unwrap().contains("<br><table"));
    let s = render(&State {
        folder: Some("gone".into()),
        ..Default::default()
    });
    assert!(s.text.contains("Папка «gone» не найдена"));
    let s = config_ui::render(
        &[module()],
        &state("secret"),
        Ui {
            english: true,
            premium: true,
        },
    )
    .unwrap();
    assert!(s.text.contains("Now"));
    assert!(!s.text.contains("sensitive-value"));
    assert!(
        s.button_appearance
            .iter()
            .flatten()
            .any(|a| a.emoji_id == Some(5807692706507399432))
    );
}

#[test]
fn fallback_number_row_retains_disabled_bounds_and_current_value() {
    let s = render(&state("count"));
    let rows = s.config_fallback.as_ref().unwrap();
    let row = rows
        .iter()
        .find(|r| r.iter().any(|k| k.text == "−10"))
        .unwrap();
    assert_eq!(
        row.iter().map(|k| k.text.as_str()).collect::<Vec<_>>(),
        vec!["−10", "−1", "0", "+1", "+10"]
    );
    assert!(matches!(row[0].action, Some(Action::Disabled)));
    assert!(matches!(row[2].action, Some(Action::Disabled)));
    assert_eq!(
        config_ui::config_value(serde_json::Value::Null, Some(&Validator::Boolean)),
        json!(false)
    );
    assert_eq!(
        config_ui::literal(&format!("{}0{}", "[".repeat(500), "]".repeat(500)))
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
