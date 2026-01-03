use clap::{Parser, Subcommand};

use crate::build::CLAP_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Cli {
    #[command(subcommand)]
    pub command: VeritySubcommand,

    #[clap(long, short = 'c', global = true)]
    /// Path to the root directory where to load configuration files. Default value is /etc/cryptpilot.
    pub config_dir: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum VeritySubcommand {
    /// Format and calculate reference values (hashes) for a given data directory.
    #[command(name = "format")]
    Format(FormatOptions),

    /// Verify the integrity of a data directory against reference values.
    #[command(name = "verify")]
    Verify(VerifyOptions),

    /// Dump metadata or root hash of a data directory.
    #[command(name = "dump")]
    Dump(DumpOptions),
}

#[derive(Parser, Debug)]
pub struct FormatOptions {
    /// Path to the data directory to calculate reference values for
    #[arg()]
    pub data_dir: std::path::PathBuf,

    /// Output file path for the metadata JSON result
    #[arg(short, long)]
    pub metadata: Option<std::path::PathBuf>,

    /// Output file path for the root hash ("-" for stdout)
    #[arg(long)]
    pub hash_output: std::path::PathBuf,
}

#[derive(Parser, Debug)]
pub struct VerifyOptions {
    /// Path to the data directory to verify
    #[arg()]
    pub data_dir: std::path::PathBuf,

    /// Expected root hash for verification
    #[arg()]
    pub hash: String,

    /// Path to the metadata JSON file
    #[arg(short, long)]
    pub metadata: Option<std::path::PathBuf>,
}

#[derive(Parser, Debug)]
pub struct DumpOptions {
    /// Path to the data directory
    #[arg(long, required_unless_present = "metadata")]
    pub data_dir: Option<std::path::PathBuf>,

    /// Path to the metadata JSON file
    #[arg(long, required_unless_present = "data_dir")]
    pub metadata: Option<std::path::PathBuf>,

    /// Print full metadata
    #[arg(long, required_unless_present = "print_root_hash")]
    pub print_metadata: bool,

    /// Print only the root hash instead of full metadata
    #[arg(long, required_unless_present = "print_metadata")]
    pub print_root_hash: bool,
}
