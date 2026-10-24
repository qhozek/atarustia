use anyhow::{Result, ensure};
use std::collections::{HashMap, HashSet};

#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    Ignore,
    Escape(String),
    Command { name: String, args: String },
}
pub fn parse(text: &str, prefix: &str, username: Option<&str>, outgoing: bool) -> Parsed {
    if prefix.is_empty() {
        return Parsed::Ignore;
    }
    if outgoing && text.starts_with(&prefix.repeat(2)) {
        return Parsed::Escape(text[prefix.len()..].to_owned());
    }
    let Some(body) = text.strip_prefix(prefix) else {
        return Parsed::Ignore;
    };
    let body = body.trim_start();
    let (head, args) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
    if head.is_empty() {
        return Parsed::Ignore;
    }
    let (name, mention) = head
        .split_once('@')
        .map_or((head, None), |(a, b)| (a, Some(b)));
    if let Some(mention) = mention {
        let own = username.is_some_and(|u| u.eq_ignore_ascii_case(mention))
            || (outgoing && mention == "me");
        if !own {
            return Parsed::Ignore;
        }
    }
    Parsed::Command {
        name: name.to_lowercase(),
        args: args.trim().into(),
    }
}
pub fn switch_layout(text: &str) -> String {
    const RU: &str = "ёйцукенгшщзхъфывапролджэячсмитьбю.ЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,";
    const EN: &str = "`qwertyuiop[]asdfghjkl;'zxcvbnm,./~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?";
    text.chars()
        .map(|c| {
            RU.chars()
                .position(|x| x == c)
                .and_then(|i| EN.chars().nth(i))
                .unwrap_or(c)
        })
        .collect()
}
#[derive(Clone, Debug)]
pub struct Grep {
    pub include: Option<String>,
    pub exclude: Option<String>,
}
pub fn grep(args: &str) -> (String, Option<Grep>) {
    if args.contains("||grep") || args.contains("|| grep") {
        return (
            args.replace("||grep", "|grep").replace("|| grep", "| grep"),
            None,
        );
    }
    let marker = args
        .rfind("| grep ")
        .map(|p| (p, 7))
        .or_else(|| args.rfind("|grep ").map(|p| (p, 6)));
    let Some((at, len)) = marker else {
        return (args.into(), None);
    };
    let needle = args[at + len..].trim();
    let (include, exclude) = if let Some(exclude) = needle.strip_prefix("-v ") {
        (None, Some(exclude.trim().into()))
    } else if let Some((include, exclude)) = needle.split_once(" -v ") {
        (Some(include.trim().into()), Some(exclude.trim().into()))
    } else {
        (Some(needle.into()), None)
    };
    (
        args[..at].trim_end().into(),
        Some(Grep { include, exclude }),
    )
}
impl Grep {
    pub fn apply(&self, text: &str) -> String {
        let lines: Vec<_> = text
            .lines()
            .filter(|line| {
                self.include.as_ref().is_none_or(|s| line.contains(s))
                    && self.exclude.as_ref().is_none_or(|s| !line.contains(s))
            })
            .collect();
        if lines.is_empty() {
            "No matching lines".into()
        } else {
            lines.join("\n")
        }
    }
}
pub fn resolve(
    name: &str,
    args: &str,
    aliases: &HashMap<String, String>,
) -> Result<(String, String)> {
    let mut name = name.to_lowercase();
    let mut args = args.to_owned();
    let mut seen = HashSet::new();
    while let Some(target) = aliases.get(&name) {
        ensure!(seen.insert(name.clone()) && seen.len() <= 16, "alias cycle");
        let (next, fixed) = target
            .split_once(char::is_whitespace)
            .unwrap_or((target, ""));
        name = next.to_lowercase();
        args = format!("{fixed} {args}").trim().into();
    }
    name = match name.as_str() {
        "cfg" => "config".into(),
        "fcfg" => "fconfig".into(),
        "eval" => "e".into(),
        _ => name,
    };
    Ok((name, args))
}
/// Token bucket; idle entries expire and capacity is bounded.
#[derive(Default)]
pub struct RateLimit {
    entries: HashMap<(i64, i64), (f64, std::time::Instant)>,
}
impl RateLimit {
    pub fn accept(&mut self, user: i64, chat: i64) -> bool {
        let now = std::time::Instant::now();
        self.entries
            .retain(|_, (_, at)| now.duration_since(*at).as_secs() < 120);
        if self.entries.len() >= 4096 && !self.entries.contains_key(&(user, chat)) {
            return false;
        }
        let (tokens, at) = self.entries.entry((user, chat)).or_insert((10.0, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64()).min(10.0);
        *at = now;
        if *tokens < 1.0 {
            false
        } else {
            *tokens -= 1.0;
            true
        }
    }
}
/// A command message is executed once even if Telegram repeats it or the bot edits its response.
#[derive(Default)]
pub struct Seen {
    entries: HashMap<(i64, i32), std::time::Instant>,
}
impl Seen {
    pub fn insert(&mut self, chat: i64, id: i32) -> bool {
        let now = std::time::Instant::now();
        self.entries
            .retain(|_, at| now.duration_since(*at) < std::time::Duration::from_secs(900));
        if self.entries.contains_key(&(chat, id)) {
            return false;
        }
        if self.entries.len() >= 4096
            && let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(k, _)| *k)
        {
            self.entries.remove(&key);
        }
        self.entries.insert((chat, id), now);
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_and_grep() {
        assert_eq!(switch_layout("зштп"), "ping");
        let (args, filter) = grep("something | grep keep -v drop");
        assert_eq!(args, "something");
        assert_eq!(filter.unwrap().apply("keep\nkeep drop\nother"), "keep");
        assert_eq!(grep("x || grep y").0, "x | grep y");
    }
    #[test]
    fn duplicate_messages_and_chat_namespaces() {
        let mut seen = Seen::default();
        assert!(seen.insert(1, 5));
        assert!(!seen.insert(1, 5));
        assert!(seen.insert(2, 5));
        assert!(seen.insert(1, 6));
        for id in 0..5000 {
            seen.insert(3, id);
        }
        assert!(seen.entries.len() <= 4096);
    }
    #[test]
    fn prefixes_and_mentions() {
        assert_eq!(
            parse("!!ping hi", "!!", None, true),
            Parsed::Command {
                name: "ping".into(),
                args: "hi".into()
            }
        );
        assert_eq!(
            parse("....ping", "..", None, true),
            Parsed::Escape("..ping".into())
        );
        assert_eq!(
            parse(".ping@other", ".", Some("mine"), true),
            Parsed::Ignore
        );
        assert_eq!(
            parse(".PING@Mine", ".", Some("mine"), false),
            Parsed::Command {
                name: "ping".into(),
                args: String::new()
            }
        );
        assert_eq!(parse(".x", "", None, true), Parsed::Ignore);
    }
    #[test]
    fn aliases_and_cycles() {
        let mut a = HashMap::from([("p".into(), "ping -x".into())]);
        assert_eq!(
            resolve("p", "hello", &a).unwrap(),
            ("ping".into(), "-x hello".into())
        );
        a.insert("ping".into(), "p".into());
        assert!(resolve("p", "", &a).is_err());
    }
    #[test]
    fn limits() {
        let mut r = RateLimit::default();
        for _ in 0..10 {
            assert!(r.accept(1, 1));
        }
        assert!(!r.accept(1, 1));
        assert!(r.accept(2, 1));
    }
}
