//! cargo run --example native_module -- --no-web --data-root ./data
use atarustia::sdk::{Command, Module, ModuleFuture, Response, Services};
use grammers_client::message::Message;
use std::sync::Arc;
struct Example;
impl Module for Example {
    fn name(&self) -> &str {
        "native_example"
    }
    fn description(&self) -> &str {
        "A statically registered Rust module"
    }
    fn commands(&self) -> Vec<Command> {
        vec![Command {
            name: "rusthello".into(),
            description: "[name] — greeting from native Rust".into(),
            permission: atarustia::security::Permission::Owner,
        }]
    }
    fn command<'a>(
        &'a self,
        _services: &'a Services,
        _message: &'a Message,
        _name: &'a str,
        args: &'a str,
    ) -> ModuleFuture<'a, Response> {
        Box::pin(async move {
            Ok(Response::Text(format!(
                "Hello from Rust, {}!",
                if args.is_empty() { "world" } else { args }
            )))
        })
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    let mut modules = atarustia::sdk::Registry::default();
    modules.register(Arc::new(Example))?;
    let restart = atarustia::runtime::run_with_modules(
        atarustia::config::Options::parse(),
        Default::default(),
        Arc::new(modules),
    )
    .await?;
    if restart {
        eprintln!(
            "Restart requested. Run this example again to keep its static native module loaded."
        );
    }
    Ok(())
}
