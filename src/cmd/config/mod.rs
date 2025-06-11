use super::{Command, IntoCommand};

pub mod check;

impl IntoCommand for crate::cli::ConfigOptions {
    fn into_command(self) -> Box<dyn Command> {
        match self.command {
            crate::cli::ConfigSubcommand::Check(config_check_options) => {
                Box::new(check::ConfigCheckCommand {
                    config_check_options,
                })
            }
        }
    }
}
