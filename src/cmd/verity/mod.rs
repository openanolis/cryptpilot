use crate::cmd::{Command, IntoCommand};

mod dump;
mod format;
mod verify;

impl IntoCommand for crate::cli::VerityOptions {
    fn into_command(self) -> Box<dyn Command> {
        match self.command {
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
