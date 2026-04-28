pub mod boot_service;
pub mod config;
pub mod show_reference_value;

use anyhow::Result;
use async_trait::async_trait;

use crate::cli::FdeSubcommand;

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

#[allow(dead_code)]
pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for FdeSubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            FdeSubcommand::ShowReferenceValue(opts) => {
                Box::new(show_reference_value::ShowReferenceValueCommand {
                    disk: opts.disk,
                    hash_algos: opts.hash_algos,
                })
            }
            FdeSubcommand::Config(config_options) => match config_options.command {
                crate::cli::ConfigSubcommand::Check(opts) => {
                    Box::new(config::check::ConfigCheckCommand {
                        config_check_options: opts,
                    })
                }
                crate::cli::ConfigSubcommand::Dump(opts) => {
                    Box::new(config::dump::ConfigDumpCommand { disk: opts.disk })
                }
            },
        }
    }
}

/// Standalone command struct for the guest boot-service binary.
/// Used by bin/guest.rs which has its own CLI struct.
pub struct GuestBootServiceCommand {
    pub boot_service_options: crate::cli::BootServiceOptions,
}

#[async_trait]
impl Command for GuestBootServiceCommand {
    async fn run(&self) -> Result<()> {
        boot_service::BootServiceCommand {
            boot_service_options: self.boot_service_options.clone(),
        }
        .run()
        .await
    }
}
