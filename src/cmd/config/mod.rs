use super::{Command, IntoCommand};

pub mod check;
pub mod dump;

impl IntoCommand for crate::cli::ConfigSubcommand {
    fn into_command(self) -> Box<dyn Command> {
        match self {
            crate::cli::ConfigSubcommand::Dump => Box::new(dump::ConfigDumpCommand {}),
            crate::cli::ConfigSubcommand::Check(config_check_options) => {
                Box::new(check::ConfigCheckCommand {
                    config_check_options,
                })
            }
        }
    }
}
