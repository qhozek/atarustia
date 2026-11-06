use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    let logs = atarustia::logs::LogBuffer::default();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "atarustia=info,grammers=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(logs.clone()),
        )
        .init();
    let restart = atarustia::runtime::run(atarustia::config::Options::parse(), logs).await?;
    if restart {
        // Update already linked this alternate executable before stopping accounts.
        // A plain restart reuses the current binary and does not invoke Cargo again.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let next = atarustia::updater::built_executable(root);
        let current = std::env::current_exe()?;
        let options = atarustia::config::Options::parse();
        let config = atarustia::config::Config::load(&options.data_root)?;
        let mut updated = false;
        for account in config.accounts.values() {
            if let Some(owner) = account.owner_id {
                let db = atarustia::database::Database::open(&options.data_root, owner)?;
                updated |= next.to_str().is_some_and(|path| {
                    db.get("RustUpdater", "next_executable").as_str() == Some(path)
                });
                db.set("RustUpdater", "next_executable", serde_json::Value::Null)?;
            }
        }
        let executable = if updated {
            anyhow::ensure!(next.is_file(), "compiled update executable is missing");
            next
        } else {
            current
        };
        let mut command = std::process::Command::new(executable);
        command.current_dir(std::env::current_dir()?);
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            return Err(command.args(args).exec().into());
        }
        #[cfg(not(unix))]
        {
            command.args(args).spawn()?;
        }
    }
    Ok(())
}
