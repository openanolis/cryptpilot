use anyhow::Result;
use async_trait::async_trait;

mod dump;
mod format;
mod verify;

#[async_trait]
pub trait Command {
    async fn run(&self) -> Result<()>;
}

pub trait IntoCommand {
    fn into_command(self) -> Box<dyn Command>;
}

impl IntoCommand for crate::cli::VeritySubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::VeritySubcommand::Format(format_options) => {
                Box::new(format::FormatCommand {
                    options: format_options,
                })
            }
            crate::cli::VeritySubcommand::Verify(verify_options) => {
                Box::new(verify::VerifyCommand {
                    options: verify_options,
                })
            }
            crate::cli::VeritySubcommand::Dump(dump_options) => Box::new(dump::DumpCommand {
                options: dump_options,
            }),
        }
    }
}
