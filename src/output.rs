pub const MESSAGE_LIMIT: usize = 3900;
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
pub fn pages(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut result = Vec::new();
    let mut page = String::new();
    let mut units = 0;
    for ch in text.chars() {
        let width = ch.len_utf16();
        if units + width > MESSAGE_LIMIT {
            result.push(std::mem::take(&mut page));
            units = 0;
        }
        page.push(ch);
        units += width;
    }
    if !page.is_empty() {
        result.push(page);
    }
    result
}
pub fn censor(mut text: String, secrets: &[String]) -> String {
    let mut secrets: Vec<_> = secrets.iter().filter(|s| s.len() >= 6).collect();
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for s in secrets {
        text = text.replace(s, "<redacted>");
    }
    text
}
pub fn error_message(error: &str, secrets: &[String]) -> String {
    let error = censor(error.to_owned(), secrets);
    let mut chars = error.chars();
    let mut short: String = chars.by_ref().take(2000).collect();
    if chars.next().is_some() {
        short.push('…');
    }
    short
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_pages() {
        let t = "🦀привет".repeat(2000);
        let p = pages(&t);
        assert_eq!(p.concat(), t);
        assert!(p.iter().all(|p| p.encode_utf16().count() <= MESSAGE_LIMIT));
    }
    #[test]
    fn redaction() {
        assert_eq!(
            censor("abcsecret123xyz".into(), &["secret123".into()]),
            "abc<redacted>xyz"
        );
    }
}
