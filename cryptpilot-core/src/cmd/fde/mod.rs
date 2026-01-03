use super::{Command, IntoCommand};

mod disk;
pub mod dump_config;
pub mod show_reference_value;

impl IntoCommand for crate::cli::FdeOptions {
    fn into_command(self) -> Box<dyn Command> {
        match self.command {
            crate::cli::FdeSubcommand::DumpConfig => {
                Box::new(dump_config::ConfigDumpCommand { disk: self.disk })
            }
            crate::cli::FdeSubcommand::ShowReferenceValue { hash_algos } => {
                Box::new(show_reference_value::ShowReferenceValueCommand {
                    disk: self.disk,
                    hash_algos,
                })
            }
        }
    }
}
