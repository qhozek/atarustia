//! First-run app selection. Telegram credentials are validated before creating a client.
use crate::config::Credentials;
use anyhow::{Context, Result};

pub fn parse_app(id_and_hash: &str, hash: Option<&str>) -> Result<Credentials> {
    let normalized = id_and_hash.replace(':', " ");
    let mut parts = normalized.split_whitespace();
    let api_id = parts.next().context("API ID is required")?.parse::<i32>()?;
    let embedded = parts.next();
    anyhow::ensure!(
        hash.is_none() || embedded.is_none(),
        "API hash was provided twice"
    );
    let api_hash = hash
        .or(embedded)
        .context("API hash is required")?
        .trim()
        .to_ascii_lowercase();
    anyhow::ensure!(parts.next().is_none(), "expected API ID and hash only");
    let credentials = Credentials { api_id, api_hash };
    credentials.validate()?;
    Ok(credentials)
}
pub async fn terminal_app(preset: &str) -> Result<Credentials> {
    let own = if preset == "ask" {
        println!("\nTelegram app\n1. Telegram Desktop\n2. My own app (my.telegram.org/apps)");
        loop {
            match crate::auth::prompt("Choose [1]: ", false).await?.as_str() {
                "" | "1" => break false,
                "2" => break true,
                _ => println!("Enter 1 or 2."),
            }
        }
    } else {
        preset == "custom"
    };
    if !own {
        return Ok(crate::config::desktop_credentials());
    }
    println!(
        "Create an app at https://my.telegram.org/apps. Paste ID and hash together, or enter them separately."
    );
    loop {
        let value =
            zeroize::Zeroizing::new(crate::auth::prompt("API ID (or ID:hash): ", false).await?);
        let normalized = value.replace(':', " ");
        let separate = if normalized.split_whitespace().count() == 1 {
            Some(zeroize::Zeroizing::new(
                crate::auth::prompt("API hash: ", true).await?,
            ))
        } else {
            None
        };
        match parse_app(&value, separate.as_ref().map(|s| s.as_str())) {
            Ok(credentials) => return Ok(credentials),
            Err(_) => println!(
                "API ID must be positive; API hash must contain 32 hexadecimal characters."
            ),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api_pair_accepts_colon_space_or_separate_fields() {
        for text in [
            "123:ABCDEF0123456789ABCDEF0123456789",
            "123 ABCDEF0123456789ABCDEF0123456789",
        ] {
            assert_eq!(
                parse_app(text, None).unwrap().api_hash,
                "abcdef0123456789abcdef0123456789"
            );
        }
        assert_eq!(parse_app("123", Some(&"a".repeat(32))).unwrap().api_id, 123);
        assert!(parse_app("0", Some(&"a".repeat(32))).is_err());
        assert!(parse_app("123 abc extra", None).is_err());
    }
}
