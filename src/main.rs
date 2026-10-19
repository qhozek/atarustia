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
        let mut command = std::process::Command::new("cargo");
        command
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["run", "--locked", "--"]);
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
