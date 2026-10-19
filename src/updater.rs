//! Source checkout updater: no binary downloads, no reset, no user changes discarded.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
pub const REPOSITORY: &str = "https://github.com/qhozek/atarustia.git";
pub static MAINTENANCE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn validate_origin(root: &Path) -> Result<()> {
    validate_origin_url(root, REPOSITORY).await
}
async fn validate_origin_url(root: &Path, expected: &str) -> Result<()> {
    let url = git(root, &["remote", "get-url", "origin"]).await?;
    ensure!(
        url == expected
            || expected == REPOSITORY
                && matches!(
                    url.trim_end_matches('/'),
                    "https://github.com/qhozek/atarustia.git"
                        | "https://github.com/qhozek/atarustia"
                        | "git@github.com:qhozek/atarustia.git"
                ),
        "origin is not the atarustia repository; refusing to change update source"
    );
    Ok(())
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Changes {
    pub commits: Vec<Commit>,
    pub total: u64,
    pub files: u64,
    pub plus: u64,
    pub minus: u64,
    pub date: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub branch: String,
    pub current: String,
    pub incoming: String,
    pub commits: String,
    pub diff: String,
    pub changes: Changes,
}
async fn git(root: &Path, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = run(&mut command, root, Duration::from_secs(40)).await?;
    ensure!(
        output.0,
        "git {} failed: {}",
        args.first().copied().unwrap_or(""),
        format!("{}{}", output.1, output.2)
            .chars()
            .take(1500)
            .collect::<String>()
    );
    ensure!(
        output.1.len() < 256 * 1024,
        "git output exceeds the updater limit; refusing a partial report"
    );
    Ok(output.1.trim().into())
}
pub async fn check(root: &Path) -> Result<Report> {
    check_from(root, REPOSITORY).await
}
async fn check_from(root: &Path, origin: &str) -> Result<Report> {
    git(root, &["rev-parse", "--show-toplevel"]).await?;
    validate_origin_url(root, origin).await?;
    let branch = git(root, &["branch", "--show-current"]).await?;
    ensure!(
        !branch.is_empty() && !branch.starts_with('-'),
        "detached HEAD; switch to a tracked branch before updating"
    );
    git(root, &["check-ref-format", "--branch", &branch]).await?;
    git(root, &["fetch", "--quiet", "origin"]).await?;
    let current = git(root, &["rev-parse", "HEAD"]).await?;
    let incoming = git(
        root,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/remotes/origin/{branch}^{{commit}}"),
        ],
    )
    .await?;
    let changes = changes(root, &current, &incoming).await?;
    let commits = changes
        .commits
        .iter()
        .map(|c| format!("{} {}", &c.hash[..7], c.subject))
        .collect::<Vec<_>>()
        .join("\n");
    let diff = git(root, &["diff", "--stat", &current, &incoming]).await?;
    Ok(Report {
        branch,
        current,
        incoming,
        commits,
        diff,
        changes,
    })
}
pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit())
}
pub async fn head(root: &Path) -> Result<String> {
    git(root, &["rev-parse", "HEAD"]).await
}
pub async fn verify_retry(root: &Path, expected: &str) -> Result<()> {
    ensure!(valid_hash(expected), "invalid retry commit");
    validate_origin(root).await?;
    ensure!(
        head(root).await? == expected,
        "checkout changed since dependency failure; review .update again"
    );
    ensure!(
        git(root, &["status", "--porcelain"]).await?.is_empty(),
        "checkout has local changes; refusing to retry"
    );
    Ok(())
}
pub async fn apply_reviewed(root: &Path, current: &str, expected: &str) -> Result<()> {
    ensure!(
        valid_hash(current) && head(root).await? == current,
        "checkout changed; review .update again"
    );
    apply_exact(root, expected, REPOSITORY, Some(current)).await
}
pub async fn changes(root: &Path, old: &str, new: &str) -> Result<Changes> {
    ensure!(
        valid_hash(old) && valid_hash(new),
        "invalid commit identity"
    );
    let span = format!("{old}..{new}");
    let log = git(root, &["log", &span, "--format=%H%x1f%s", "--max-count=15"]).await?;
    let commits = log
        .lines()
        .filter_map(|line| line.split_once('\x1f'))
        .filter(|(hash, _)| valid_hash(hash))
        .map(|(hash, subject)| Commit {
            hash: hash.into(),
            subject: subject.chars().take(300).collect(),
        })
        .collect();
    let total = git(root, &["rev-list", "--count", &span]).await?.parse()?;
    let stat = git(root, &["diff", "--numstat", old, new]).await?;
    let mut result = Changes {
        commits,
        total,
        date: git(root, &["show", "-s", "--format=%ct", new])
            .await?
            .parse()?,
        ..Default::default()
    };
    for line in stat.lines() {
        let mut parts = line.splitn(3, '\t');
        if let (Some(plus), Some(minus), Some(_)) = (parts.next(), parts.next(), parts.next()) {
            result.files += 1;
            result.plus += plus.parse::<u64>().unwrap_or(0);
            result.minus += minus.parse::<u64>().unwrap_or(0);
        }
    }
    Ok(result)
}

/// The reviewed identity is rechecked immediately before an exact fast-forward. No reset or pull.
pub async fn apply(root: &Path, expected: &str) -> Result<()> {
    apply_from(root, expected, REPOSITORY).await
}
async fn apply_from(root: &Path, expected: &str, origin: &str) -> Result<()> {
    apply_exact(root, expected, origin, None).await
}
async fn apply_exact(
    root: &Path,
    expected: &str,
    origin: &str,
    current: Option<&str>,
) -> Result<()> {
    ensure!(
        valid_hash(expected),
        "provide full incoming commit hash from .update"
    );
    let status = git(root, &["status", "--porcelain"]).await?;
    ensure!(
        status.is_empty(),
        "checkout has local changes; refusing to update"
    );
    let report = check_from(root, origin).await?;
    ensure!(
        report.incoming == expected,
        "upstream changed; review .update again"
    );
    ensure!(
        current.is_none_or(|current| current == report.current),
        "checkout changed; review .update again"
    );
    git(root, &["merge-base", "--is-ancestor", "HEAD", expected])
        .await
        .context("upstream diverged; refusing to reset local history")?;
    git(root, &["merge", "--ff-only", expected]).await?;
    // Cargo compiles from source after restart. No prebuilt/release artifact is created here.
    Ok(())
}

/// Drain both pipes with bounded retained output; compiler verbosity cannot grow memory without bound.
async fn run(
    command: &mut tokio::process::Command,
    root: &Path,
    timeout: Duration,
) -> Result<(bool, String, String)> {
    use tokio::io::AsyncReadExt;
    command
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn()?;
    #[cfg(windows)]
    let _job = crate::commands::WindowsJob::attach(&child)?;
    let _group = crate::commands::ProcessGroup(child.id());
    let read = |mut stream: Box<dyn tokio::io::AsyncRead + Send + Unpin>| async move {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            let count = stream.read(&mut buffer).await?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.len() > 256 * 1024 {
                bytes.drain(..bytes.len() - 256 * 1024);
            }
        }
        Ok::<_, anyhow::Error>(bytes)
    };
    let stdout = child.stdout.take().context("stdout missing")?;
    let stderr = child.stderr.take().context("stderr missing")?;
    let task = async {
        let (out, err, status) =
            tokio::try_join!(read(Box::new(stdout)), read(Box::new(stderr)), async {
                Ok::<_, anyhow::Error>(child.wait().await?)
            })?;
        Ok::<_, anyhow::Error>((
            status.success(),
            String::from_utf8_lossy(&out).to_string(),
            String::from_utf8_lossy(&err).to_string(),
        ))
    };
    // Keep process ownership inside the timeout so cancellation kills Cargo and its children as well.
    tokio::time::timeout(timeout, task)
        .await
        .context("update process timed out")?
}
pub async fn sync(root: &Path) -> Result<()> {
    let mut command = tokio::process::Command::new("cargo");
    command.args(["check", "--locked", "--bin", "atarustia"]);
    let (success, output, errors) = run(&mut command, root, Duration::from_secs(300)).await?;
    ensure!(success, "{output}{errors}");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn shell(script: &str) -> tokio::process::Command {
        #[cfg(windows)]
        {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", script]);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", script]);
            c
        }
    }
    #[tokio::test]
    async fn updater_process_separates_output_and_terminates_on_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let (success, out, err) = run(
            &mut shell("echo output & echo diagnostic 1>&2"),
            dir.path(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert!(success);
        assert!(out.contains("output"));
        assert!(err.contains("diagnostic"));
        assert!(!out.contains("diagnostic"));
        #[cfg(windows)]
        let script = "ping -n 10 127.0.0.1 >nul";
        #[cfg(not(windows))]
        let script = "sleep 10";
        assert!(
            run(&mut shell(script), dir.path(), Duration::from_millis(100))
                .await
                .unwrap_err()
                .to_string()
                .contains("timed out")
        );
    }
    struct Checkout {
        _dir: tempfile::TempDir,
        local: std::path::PathBuf,
        remote: std::path::PathBuf,
    }
    impl Checkout {
        async fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let remote = dir.path().join("remote");
            let local = dir.path().join("local");
            std::fs::create_dir(&remote).unwrap();
            git(&remote, &["init", "-b", "main"]).await.unwrap();
            git(&remote, &["config", "user.name", "Fixture"])
                .await
                .unwrap();
            git(
                &remote,
                &["config", "user.email", "fixture@example.invalid"],
            )
            .await
            .unwrap();
            git(&remote, &["config", "core.autocrlf", "false"])
                .await
                .unwrap();
            std::fs::write(remote.join("source"), "one\ntwo\n").unwrap();
            git(&remote, &["add", "."]).await.unwrap();
            git(&remote, &["commit", "-m", "initial"]).await.unwrap();
            git(
                dir.path(),
                &[
                    "clone",
                    "--config",
                    "core.autocrlf=false",
                    remote.to_str().unwrap(),
                    local.to_str().unwrap(),
                ],
            )
            .await
            .unwrap();
            git(&local, &["config", "core.autocrlf", "false"])
                .await
                .unwrap();
            Self {
                _dir: dir,
                local,
                remote,
            }
        }
        async fn next(&self) -> String {
            std::fs::write(self.remote.join("source"), "one\nthree\nfour\n").unwrap();
            git(&self.remote, &["add", "."]).await.unwrap();
            git(&self.remote, &["commit", "-m", "fix <unsafe> {branch}"])
                .await
                .unwrap();
            head(&self.remote).await.unwrap()
        }
    }
    #[tokio::test]
    async fn local_git_update_collects_original_stats_and_applies_exact_commit() {
        let f = Checkout::new().await;
        let old = head(&f.local).await.unwrap();
        let new = f.next().await;
        let report = check_from(&f.local, f.remote.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(report.current, old);
        assert_eq!(report.incoming, new);
        assert_eq!(report.branch, "main");
        assert_eq!(report.changes.total, 1);
        assert_eq!(report.changes.files, 1);
        assert_eq!(report.changes.plus, 2);
        assert_eq!(report.changes.minus, 1);
        assert!(report.changes.date > 0);
        assert_eq!(report.changes.commits[0].subject, "fix <unsafe> {branch}");
        let error = apply_exact(
            &f.local,
            &new,
            f.remote.to_str().unwrap(),
            Some(&"f".repeat(40)),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("checkout changed"));
        assert_eq!(head(&f.local).await.unwrap(), old);
        apply_from(&f.local, &new, f.remote.to_str().unwrap())
            .await
            .unwrap();
        assert_eq!(head(&f.local).await.unwrap(), new);
        assert_eq!(
            std::fs::read_to_string(f.local.join("source")).unwrap(),
            "one\nthree\nfour\n"
        );
    }
    #[tokio::test]
    async fn dirty_and_stale_checkout_never_discards_files_or_unreviewed_updates() {
        let f = Checkout::new().await;
        let old = head(&f.local).await.unwrap();
        let reviewed = f.next().await;
        std::fs::write(f.local.join("mine"), "keep").unwrap();
        assert!(
            apply_from(&f.local, &reviewed, f.remote.to_str().unwrap())
                .await
                .unwrap_err()
                .to_string()
                .contains("local changes")
        );
        assert_eq!(head(&f.local).await.unwrap(), old);
        assert_eq!(
            std::fs::read_to_string(f.local.join("mine")).unwrap(),
            "keep"
        );
        // Include the previously untracked file without deleting it.
        git(&f.local, &["add", "mine"]).await.unwrap();
        git(&f.local, &["config", "user.name", "Fixture"])
            .await
            .unwrap();
        git(
            &f.local,
            &["config", "user.email", "fixture@example.invalid"],
        )
        .await
        .unwrap();
        git(&f.local, &["commit", "-m", "local work"])
            .await
            .unwrap();
        assert!(
            apply_from(&f.local, &reviewed, f.remote.to_str().unwrap())
                .await
                .unwrap_err()
                .to_string()
                .contains("diverged")
        );
        assert_eq!(
            std::fs::read_to_string(f.local.join("mine")).unwrap(),
            "keep"
        );
        std::fs::write(f.remote.join("newer"), "not reviewed").unwrap();
        git(&f.remote, &["add", "."]).await.unwrap();
        git(&f.remote, &["commit", "-m", "newer remote"])
            .await
            .unwrap();
        assert!(
            apply_from(&f.local, &reviewed, f.remote.to_str().unwrap())
                .await
                .unwrap_err()
                .to_string()
                .contains("upstream changed")
        );
    }
    #[tokio::test]
    async fn history_limit_retains_total_and_binary_files_count() {
        let f = Checkout::new().await;
        let old = head(&f.remote).await.unwrap();
        for i in 0..17 {
            std::fs::write(f.remote.join("binary"), [0, i]).unwrap();
            git(&f.remote, &["add", "."]).await.unwrap();
            git(&f.remote, &["commit", "-m", &format!("change {i}")])
                .await
                .unwrap();
        }
        let value = changes(&f.remote, &old, &head(&f.remote).await.unwrap())
            .await
            .unwrap();
        assert_eq!(value.total, 17);
        assert_eq!(value.commits.len(), 15);
        assert_eq!(value.files, 1);
        assert_eq!(value.plus, 0);
        assert_eq!(value.minus, 0);
    }
    #[tokio::test]
    async fn dependency_failure_does_not_require_telegram_or_restart() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='atarustia'\nversion='0.0.0'\nedition='2024'\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("Cargo.lock"),
            "version = 4\n[[package]]\nname = 'atarustia'\nversion = '0.0.0'\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() { syntax error }").unwrap();
        let error = sync(dir.path()).await.unwrap_err().to_string();
        assert!(error.contains("error"));
        assert!(dir.path().join("src/main.rs").is_file());
        assert!(!dir.path().join("target/release").exists());
    }
    #[tokio::test]
    async fn wrong_origin_is_rejected_before_fetch() {
        let d = tempfile::tempdir().unwrap();
        git(d.path(), &["init", "-b", "main"]).await.unwrap();
        git(
            d.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/not-this-project.git",
            ],
        )
        .await
        .unwrap();
        let error = check(d.path()).await.err().unwrap().to_string();
        assert!(error.contains("origin is not"));
    }
    #[tokio::test]
    async fn never_resets_an_unversioned_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("mine"), "keep").unwrap();
        assert!(apply(d.path(), &"0".repeat(40)).await.is_err());
        assert_eq!(
            std::fs::read_to_string(d.path().join("mine")).unwrap(),
            "keep"
        );
    }
}
