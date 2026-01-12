use clap::{Args, Parser, Subcommand, ValueEnum};
use std::fmt::Display;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CryptSubcommand,

    /// Path to the root directory where to load configuration files. Default value is /etc/cryptpilot.
    #[clap(long, short = 'c', global = true)]
    pub config_dir: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum CryptSubcommand {
    /// Show status about volumes.
    #[command(name = "show")]
    Show(ShowOptions),

    /// Initialize a new volume.
    #[command(name = "init")]
    Init(InitOptions),

    /// Open an existing volume.
    #[command(name = "open")]
    Open(OpenOptions),

    /// Close an open volume.
    #[command(name = "close")]
    Close(CloseOptions),

    /// Subcommands related to configuration.
    #[command(name = "config")]
    Config(ConfigOptions),

    /// Running during system booting for data volumes auto-open.
    #[command(name = "boot-service")]
    BootService(BootServiceOptions),
}

#[derive(Parser, Debug)]
pub struct ShowOptions {
    /// Name of the volume(s) to show. If not specified, show all volumes.
    #[arg(num_args=0..)]
    pub volume: Vec<String>,

    /// Output as JSON format instead of table
    #[clap(long)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct InitOptions {
    /// Name of the volume to initialize.
    #[arg(required=true, num_args=1..)]
    pub volume: Vec<String>,

    /// Force re-initialization of the volume.
    #[clap(long, default_value = "false")]
    pub force_reinit: bool,

    /// Skip confirmation prompts.
    #[clap(long, short = 'y', default_value = "false")]
    pub yes: bool,
}

#[derive(Parser, Debug)]
pub struct OpenOptions {
    /// Name of the volume to open.
    #[arg(required=true, num_args=1..)]
    pub volume: Vec<String>,
}

#[derive(Parser, Debug)]
pub struct CloseOptions {
    /// Name of the volume to close.
    #[arg(required=true, num_args=1..)]
    pub volume: Vec<String>,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ConfigOptions {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Subcommand, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub enum ConfigSubcommand {
    /// Check if the config is valid.
    #[command(name = "check")]
    Check(ConfigCheckOptions),
}

#[derive(Parser, Debug)]
pub struct ConfigCheckOptions {
    /// Keep checking the config even if one of the config is invalid.
    #[clap(long)]
    pub keep_checking: bool,

    /// Skip verifing for fetching the encryption key from the configed key provider.
    #[clap(long)]
    pub skip_check_passphrase: bool,
}

#[derive(Parser, Debug)]
pub struct BootServiceOptions {
    /// Indicate the stage of the boot process we are in.
    #[clap(long)]
    #[arg(value_enum)]
    pub stage: BootStage,
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum BootStage {
    #[clap(name = "system-volumes-auto-open")]
    SystemVolumesAutoOpen,
}

impl Display for BootStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootStage::SystemVolumesAutoOpen => write!(f, "system-volumes-auto-open"),
        }
    }
}
