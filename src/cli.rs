use std::{fmt::Display, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::build::CLAP_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Cli {
    #[command(subcommand)]
    pub command: GlobalSubcommand,

    #[clap(long, short = 'c', global = true)]
    /// Path to the root directory where to load configuration files. Default value is /etc/cryptpilot.
    pub config_dir: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum GlobalSubcommand {
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

    #[command(name = "fde")]
    Fde(FdeOptions),

    /// Running during system booting (both initrd stage and system stage).
    #[command(name = "boot-service")]
    BootService(BootServiceOptions),
}

#[derive(Parser, Debug)]
pub struct ShowOptions {}

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

#[derive(Debug, Args)]
pub struct FdeOptions {
    #[command(subcommand)]
    pub command: FdeSubcommand,

    /// Operate on the specified disk instead of the running system. The path can be a file or block device.
    #[clap(long, global = true)]
    pub disk: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum FdeSubcommand {
    /// Display cryptographic reference values (e.g., hashes) of boot-related components for attestation.
    ///
    /// This includes artifacts such as:
    /// - GRUB configuration and binaries
    /// - Shim and bootloader
    /// - Initrd and kernel images
    /// - Kernel command line
    ///
    /// For encrypted (FDE) disks, additional values are included:
    /// - Root filesystem hash (integrity measurement)
    /// - Cryptpilot configuration bundle hash
    ///
    /// Supports both encrypted (FDE) and plain disks. Optionally filtered by stage.
    #[command(name = "show-reference-value")]
    ShowReferenceValue {
        /// Specify one or more hash algorithms to use.
        /// Multiple algorithms can be provided (e.g., --hash-algo sha384 --hash-algo sm3).
        #[clap(long = "hash-algo", default_value = "sha384,sm3")]
        hash_algos: Vec<ShowReferenceValueHashAlgo>,
        /// Optional stage filter (e.g., initrd, system). If not provided, all stages are included.
        ///
        /// Used to inject additional reference values for specific boot stages. For example:
        /// - When `--stage system` is specified, an extra claim is added:
        ///   `cryptpilot.alibabacloud.com initrd_switch_root {}`
        ///   indicating that the system has switched to the new root filesystem (sysroot).
        ///
        /// If omitted, no additional stage-specific claims are generated.
        #[clap(long)]
        stage: Option<ShowReferenceValueStage>,
    },

    /// Dump fde config and global config as toml, which can be used in cloud-init user data.
    #[command(name = "dump-config")]
    DumpConfig,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ShowReferenceValueHashAlgo {
    #[clap(name = "sha1")]
    Sha1,

    #[clap(name = "sha256")]
    Sha256,

    #[clap(name = "sha384")]
    Sha384,

    #[clap(name = "sm3")]
    Sm3,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum ShowReferenceValueStage {
    #[clap(name = "initrd")]
    Initrd,

    #[clap(name = "system")]
    System,
}

#[derive(Parser, Debug)]
pub struct BootServiceOptions {
    /// Indicate the stage of the boot process we are in.
    #[clap(long)]
    #[arg(value_enum)]
    pub stage: BootStage,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum BootStage {
    #[clap(name = "initrd-fde-before-sysroot")]
    InitrdFdeBeforeSysroot,

    #[clap(name = "initrd-fde-after-sysroot")]
    InitrdFdeAfterSysroot,

    #[clap(name = "system-volumes-auto-open")]
    SystemVolumesAutoOpen,
}

impl Display for BootStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootStage::InitrdFdeBeforeSysroot => write!(f, "initrd-fde-before-sysroot"),
            BootStage::InitrdFdeAfterSysroot => write!(f, "initrd-fde-after-sysroot"),
            BootStage::SystemVolumesAutoOpen => write!(f, "system-volumes-auto-open"),
        }
    }
}
