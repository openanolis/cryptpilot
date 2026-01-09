pub mod boot_service;
pub mod config;
pub mod show_reference_value;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    cli::{BootServiceOptions, ShowReferenceValueOptions},
    cmd::{
        boot_service::BootServiceCommand,
        config::{check::ConfigCheckCommand, dump::ConfigDumpCommand},
        show_reference_value::ShowReferenceValueCommand,
    },
};

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

#[allow(dead_code)]
pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::FdeSubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::FdeSubcommand::ShowReferenceValue(ShowReferenceValueOptions {
                disk,
                hash_algos,
            }) => Box::new(ShowReferenceValueCommand { disk, hash_algos }),
            crate::cli::FdeSubcommand::Config(config_options) => match config_options.command {
                crate::cli::ConfigSubcommand::Check(config_check_options) => {
                    Box::new(ConfigCheckCommand {
                        config_check_options,
                    })
                }
                crate::cli::ConfigSubcommand::Dump(dump_config_options) => {
                    Box::new(ConfigDumpCommand {
                        disk: dump_config_options.disk,
                    })
                }
            },
            crate::cli::FdeSubcommand::BootService(BootServiceOptions { stage }) => {
                Box::new(BootServiceCommand {
                    boot_service_options: BootServiceOptions { stage },
                })
            }
        }
    }
}
