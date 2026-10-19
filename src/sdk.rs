//! Native Rust module interface. Embed/register statically; no unsafe dynamic-library ABI.
use crate::{database::Database, forms::Screen, security::Permission};
use anyhow::Result;
use grammers_client::{Client, message::Message, update::Update};
use std::{future::Future, pin::Pin, sync::Arc};
pub type ModuleFuture<'a, T = ()> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;
#[derive(Clone)]
pub struct Services {
    pub client: Client,
    pub db: Arc<Database>,
    pub owner: i64,
    pub stop: tokio_util::sync::CancellationToken,
}
#[derive(Clone, Debug)]
pub struct Command {
    pub name: String,
    pub description: String,
    pub permission: Permission,
}
pub enum Response {
    Text(String),
    Screen(Box<Screen>),
    None,
}
/// Rust modules receive real services explicitly, rather than importing process globals.
pub trait Module: Send + Sync + 'static {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn help_metadata(&self) -> crate::help::Metadata {
        Default::default()
    }
    fn commands(&self) -> Vec<Command> {
        vec![]
    }
    fn ready<'a>(&'a self, _services: &'a Services) -> ModuleFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn command<'a>(
        &'a self,
        _services: &'a Services,
        _message: &'a Message,
        _name: &'a str,
        _args: &'a str,
    ) -> ModuleFuture<'a, Response> {
        Box::pin(async { Ok(Response::None) })
    }
    fn update<'a>(&'a self, _services: &'a Services, _update: &'a Update) -> ModuleFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn unload<'a>(&'a self, _services: &'a Services) -> ModuleFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}
#[derive(Default)]
pub struct Registry {
    modules: Vec<Arc<dyn Module>>,
}
impl Registry {
    pub fn register(&mut self, module: Arc<dyn Module>) -> Result<()> {
        module.help_metadata().validate()?;
        anyhow::ensure!(
            crate::modules::valid_name(module.name()),
            "invalid native module name"
        );
        anyhow::ensure!(
            !self.modules.iter().any(|m| m.name() == module.name()),
            "duplicate native module"
        );
        let mut names = std::collections::HashSet::new();
        for command in module.commands() {
            command.permission.validate()?;
            anyhow::ensure!(
                crate::modules::valid_name(&command.name)
                    && names.insert(command.name.clone())
                    && !crate::commands::is_builtin(&command.name)
                    && self.command(&command.name).is_none(),
                "native command collision"
            );
        }
        self.modules.push(module);
        Ok(())
    }
    pub fn list(&self) -> Vec<Arc<dyn Module>> {
        self.modules.clone()
    }
    pub fn command(&self, name: &str) -> Option<(Arc<dyn Module>, Command)> {
        self.modules.iter().find_map(|m| {
            m.commands()
                .into_iter()
                .find(|c| c.name == name)
                .map(|c| (m.clone(), c))
        })
    }
    pub async fn ready(&self, services: &Services) -> Result<()> {
        for module in &self.modules {
            module.ready(services).await?;
        }
        Ok(())
    }
    pub async fn unload(&self, services: &Services) {
        for module in self.modules.iter().rev() {
            if let Err(e) = module.unload(services).await {
                tracing::warn!(module=module.name(),error=%e,"native module unload failed");
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Demo;
    impl Module for Demo {
        fn name(&self) -> &str {
            "demo"
        }
        fn description(&self) -> &str {
            "demo"
        }
        fn commands(&self) -> Vec<Command> {
            vec![Command {
                name: "native_demo".into(),
                description: "test".into(),
                permission: Permission::Owner,
            }]
        }
    }
    #[test]
    fn native_registration_and_collision() {
        let mut registry = Registry::default();
        registry.register(Arc::new(Demo)).unwrap();
        assert!(registry.command("native_demo").is_some());
        assert!(registry.register(Arc::new(Demo)).is_err());
    }
}
