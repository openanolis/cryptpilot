use anyhow::Result;
use async_trait::async_trait;

pub mod boot_service;
pub mod close;
pub mod dump_config;
pub mod init;
pub mod open;
pub mod show;

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::Command {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::Command::Show(show_options) => Box::new(show::ShowCommand { show_options }),
            crate::cli::Command::Init(init_options) => Box::new(init::InitCommand { init_options }),
            crate::cli::Command::Open(open_options) => Box::new(open::OpenCommand { open_options }),
            crate::cli::Command::Close(close_options) => {
                Box::new(close::CloseCommand { close_options })
            }
            crate::cli::Command::DumpConfig => Box::new(dump_config::DumpConfigCommand {}),
            crate::cli::Command::BootService(boot_service_options) => {
                Box::new(boot_service::BootServiceCommand {
                    boot_service_options,
                })
            }
        }
    }
}
