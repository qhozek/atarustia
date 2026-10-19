//! Source checkout updater: no binary downloads, no reset, no user changes discarded.
use anyhow::{Context, Result, ensure};
use std::{path::Path, time::Duration};
pub const REPOSITORY: &str = "https://github.com/qhozek/atarustia.git";
async fn validate_origin(root: &Path) -> Result<()> {
    let url = git(root, &["remote", "get-url", "origin"]).await?;
    ensure!(
        matches!(
            url.trim_end_matches('/'),
            "https://github.com/qhozek/atarustia.git"
                | "https://github.com/qhozek/atarustia"
                | "git@github.com:qhozek/atarustia.git"
        ),
        "origin is not the atarustia repository; refusing to change update source"
    );
    Ok(())
}
async fn ensure_tracking(root: &Path) -> Result<()> {
    if git(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
    .await
    .is_err()
    {
        let branch = git(root, &["branch", "--show-current"]).await?;
        ensure!(branch == "main", "no upstream configured for this branch");
        git(root, &["branch", "--set-upstream-to=origin/main", "main"]).await?;
    }
    Ok(())
}
pub struct Report {
    pub current: String,
    pub incoming: String,
    pub commits: String,
    pub diff: String,
}
async fn git(root: &Path, args: &[&str]) -> Result<String> {
    let mut command = tokio::process::Command::new("git");
    command
        .args(args)
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(40), command.output())
        .await
        .context("git timed out")??;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.first().copied().unwrap_or(""),
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(1500)
            .collect::<String>()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}
pub async fn check(root: &Path) -> Result<Report> {
    git(root, &["rev-parse", "--show-toplevel"]).await?;
    validate_origin(root).await?;
    git(root, &["fetch", "--quiet", "origin"]).await?;
    ensure_tracking(root).await?;
    let current = git(root, &["rev-parse", "HEAD"]).await?;
    let incoming = git(root, &["rev-parse", "@{upstream}"]).await?;
    let commits = git(
        root,
        &[
            "log",
            "--format=%h %s",
            "--max-count=15",
            "HEAD..@{upstream}",
        ],
    )
    .await?;
    let diff = git(root, &["diff", "--stat", "HEAD..@{upstream}"]).await?;
    Ok(Report {
        current,
        incoming,
        commits,
        diff,
    })
}
pub async fn apply(root: &Path, expected: &str) -> Result<()> {
    ensure!(
        expected.len() == 40 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "provide full incoming commit hash from .update"
    );
    let status = git(root, &["status", "--porcelain"]).await?;
    ensure!(
        status.is_empty(),
        "checkout has local changes; refusing to update"
    );
    let report = check(root).await?;
    ensure!(
        report.incoming == expected,
        "upstream changed; review .update again"
    );
    git(root, &["merge-base", "--is-ancestor", "HEAD", expected])
        .await
        .context("upstream diverged; refusing to reset local history")?;
    git(root, &["merge", "--ff-only", expected]).await?;
    // Cargo compiles from source after restart. No prebuilt/release artifact is created here.
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
