pub mod boot_service;
pub mod close;
pub mod config;
pub mod init;
pub mod open;
pub mod show;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    cli::{BootServiceOptions, ConfigOptions, ConfigSubcommand},
    cmd::boot_service::BootServiceCommand,
};
use close::CloseCommand;
use config::check::ConfigCheckCommand;
use init::InitCommand;
use open::OpenCommand;
use show::ShowCommand;

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

#[allow(dead_code)]
pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::CryptSubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::CryptSubcommand::Show(show_options) => {
                Box::new(ShowCommand { show_options })
            }
            crate::cli::CryptSubcommand::Init(init_options) => {
                Box::new(InitCommand { init_options })
            }
            crate::cli::CryptSubcommand::Open(open_options) => {
                Box::new(OpenCommand { open_options })
            }
            crate::cli::CryptSubcommand::Close(close_options) => {
                Box::new(CloseCommand { close_options })
            }
            crate::cli::CryptSubcommand::Config(ConfigOptions { command }) => match command {
                ConfigSubcommand::Check(config_check_options) => Box::new(ConfigCheckCommand {
                    config_check_options,
                }),
            },
            crate::cli::CryptSubcommand::BootService(BootServiceOptions { stage }) => {
                Box::new(BootServiceCommand { stage })
            }
        }
    }
}
