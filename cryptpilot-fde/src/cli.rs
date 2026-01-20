use std::{fmt::Display, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::build::CLAP_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Cli {
    #[command(subcommand)]
    pub command: FdeSubcommand,

    /// Path to the root directory where to load configuration files. Default value is /etc/cryptpilot.
    #[clap(long, short = 'c', global = true)]
    pub config_dir: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum FdeSubcommand {
    /// Display cryptographic reference values (e.g., hashes) of boot-related artifacts for attestation.
    #[command(name = "show-reference-value")]
    ShowReferenceValue(ShowReferenceValueOptions),

    /// Subcommands related to configuration.
    #[command(name = "config")]
    Config(ConfigOptions),

    /// Running during system booting FDE stages.
    #[command(name = "boot-service")]
    BootService(BootServiceOptions),
}

#[derive(Parser, Debug)]
pub struct ShowReferenceValueOptions {
    /// Operate on the specified disk instead of the running system. The path can be a file or block device.
    #[clap(long, global = true)]
    pub disk: Option<PathBuf>,

    /// Specify one or more hash algorithms to use.
    /// Multiple algorithms can be provided (e.g., --hash-algo sha384 --hash-algo sm3).
    #[clap(long = "hash-algo", default_value = "sha384")]
    pub hash_algos: Vec<ShowReferenceValueHashAlgo>,
}

#[derive(Parser, Debug)]
pub struct ConfigDumpOptions {
    /// Operate on the specified disk instead of the running system. The path can be a file or block device.
    #[clap(long, global = true)]
    pub disk: Option<PathBuf>,
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

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ConfigOptions {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Subcommand, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub enum ConfigSubcommand {
    /// Check if the FDE config is valid.
    #[command(name = "check")]
    Check(ConfigCheckOptions),

    /// Dump fde config and global config as toml, which can be used in cloud-init user data.
    #[command(name = "dump")]
    Dump(ConfigDumpOptions),
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
    #[clap(name = "initrd-fde-before-sysroot")]
    InitrdFdeBeforeSysroot,

    #[clap(name = "initrd-fde-after-sysroot")]
    InitrdFdeAfterSysroot,
}

impl Display for BootStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootStage::InitrdFdeBeforeSysroot => write!(f, "initrd-fde-before-sysroot"),
            BootStage::InitrdFdeAfterSysroot => write!(f, "initrd-fde-after-sysroot"),
        }
    }
}
