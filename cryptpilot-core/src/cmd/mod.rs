use anyhow::Result;
use async_trait::async_trait;

pub mod boot_service;
pub mod close;
pub mod config;
pub mod fde;
pub mod init;
pub mod open;
pub mod show;
pub mod verity;

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::GlobalSubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::GlobalSubcommand::Show(show_options) => {
                Box::new(show::ShowCommand { show_options })
            }
            crate::cli::GlobalSubcommand::Init(init_options) => {
                Box::new(init::InitCommand { init_options })
            }
            crate::cli::GlobalSubcommand::Open(open_options) => {
                Box::new(open::OpenCommand { open_options })
            }
            crate::cli::GlobalSubcommand::Close(close_options) => {
                Box::new(close::CloseCommand { close_options })
            }
            crate::cli::GlobalSubcommand::Config(config_options) => config_options.into_command(),
            crate::cli::GlobalSubcommand::BootService(boot_service_options) => {
                Box::new(boot_service::BootServiceCommand {
                    boot_service_options,
                })
            }
            crate::cli::GlobalSubcommand::Fde(fde_options) => fde_options.into_command(),
            crate::cli::GlobalSubcommand::Verity(verity_options) => verity_options.into_command(),
        }
    }
}
