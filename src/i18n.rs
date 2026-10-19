//! Layered translations: external -> selected language -> English, with literal placeholders.
use crate::database::Database;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub const RU: &str = include_str!("../locales/ru.json");
pub const EN: &str = include_str!("../locales/en.json");
pub fn text(db: &Database, key: &str, values: &[(&str, String)]) -> String {
    let lang = db.language();
    let external = db.get("atarustia.translations", "external");
    let pack: Value = serde_json::from_str(if lang.split_whitespace().next() == Some("en") {
        EN
    } else {
        RU
    })
    .expect("embedded locale");
    let fallback: Value = serde_json::from_str(EN).expect("embedded locale");
    let mut text = external
        .get(key)
        .and_then(Value::as_str)
        .or_else(|| pack.get(key).and_then(Value::as_str))
        .or_else(|| fallback.get(key).and_then(Value::as_str))
        .unwrap_or(key)
        .to_owned();
    for (name, value) in values {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}
pub fn parse(bytes: &[u8], language: &str) -> Result<BTreeMap<String, String>> {
    ensure!(bytes.len() <= 1024 * 1024, "language pack exceeds 1 MiB");
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => serde_yaml::from_slice(bytes).context("invalid JSON/YAML language pack")?,
    };
    let object = value
        .as_object()
        .context("language pack must be an object")?;
    let chosen = object
        .get(language)
        .filter(|v| v.is_object())
        .unwrap_or(&value);
    let mut result = BTreeMap::new();
    fn flatten(prefix: &str, value: &Value, out: &mut BTreeMap<String, String>) -> Result<()> {
        match value {
            Value::String(s) => {
                ensure!(s.len() <= 16384, "translation too long");
                out.insert(prefix.into(), s.clone());
            }
            Value::Object(map) => {
                for (key, value) in map {
                    ensure!(key.len() <= 128, "translation key too long");
                    flatten(
                        &if prefix.is_empty() {
                            key.clone()
                        } else {
                            format!("{prefix}.{key}")
                        },
                        value,
                        out,
                    )?;
                }
            }
            _ => anyhow::bail!("translations must be strings or nested objects"),
        };
        Ok(())
    }
    flatten("", chosen, &mut result)?;
    ensure!(result.len() <= 4096, "too many translations");
    Ok(result)
}
pub async fn install(db: &Database, url: &str) -> Result<()> {
    let url = reqwest::Url::parse(url)?;
    ensure!(url.scheme() == "https", "language URL must use HTTPS");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()?;
    let mut response = client.get(url.clone()).send().await?.error_for_status()?;
    ensure!(response.url().scheme() == "https", "insecure redirect");
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(bytes.len() + chunk.len() <= 1024 * 1024, "pack too large");
        bytes.extend_from_slice(&chunk);
    }
    let pack = parse(&bytes, &db.language())?;
    db.set("atarustia.translations", "external", json!(pack))?;
    db.set("atarustia.translations", "external_url", url.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parity_and_fallback() {
        let ru: Value = serde_json::from_str(RU).unwrap();
        let en: Value = serde_json::from_str(EN).unwrap();
        assert_eq!(
            ru.as_object().unwrap().keys().collect::<Vec<_>>(),
            en.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        let d = tempfile::tempdir().unwrap();
        let db = Database::open(d.path(), 1).unwrap();
        db.set("atarustia.translations", "lang", "en").unwrap();
        assert_eq!(text(&db, "saved", &[]), "Saved");
        db.set(
            "atarustia.translations",
            "external",
            json!({"saved":"custom"}),
        )
        .unwrap();
        assert_eq!(text(&db, "saved", &[]), "custom");
    }
    #[test]
    fn nested_yaml_and_invalid() {
        let pack = parse(
            b"en:\n  screen:\n    title: Hello\nru:\n  screen:\n    title: Test\n",
            "en",
        )
        .unwrap();
        assert_eq!(pack["screen.title"], "Hello");
        assert!(parse(b"x: [1,2]", "en").is_err());
    }
}
