use clap::{Parser, Subcommand};

fn parse_label(s: &str) -> Result<(String, String), String> {
    let Some((key, value)) = s.split_once('=') else {
        return Err(format!("invalid label format '{}', expected key=value", s));
    };
    if key.is_empty() {
        return Err("label key cannot be empty".to_string());
    }
    Ok((key.to_string(), value.to_string()))
}

use crate::build::CLAP_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Cli {
    #[command(subcommand)]
    pub command: VeritySubcommand,
}

#[derive(Subcommand, Debug)]
pub enum VeritySubcommand {
    /// Format and calculate root hash for a given data directory.
    #[command(name = "format")]
    Format(FormatOptions),

    /// Verify the integrity of a data directory against root hash.
    #[command(name = "verify")]
    Verify(VerifyOptions),

    /// Dump metadata or root hash of a data directory.
    #[command(name = "dump")]
    Dump(DumpOptions),

    /// Mount a data directory as a verity-fuse filesystem.
    #[command(name = "open")]
    Open(OpenOptions),

    /// Unmount a verity-fuse filesystem.
    #[command(name = "close")]
    Close(CloseOptions),
}

#[derive(Parser, Debug)]
pub struct FormatOptions {
    /// Path to the data directory for which to calculate reference values
    #[arg()]
    pub data_dir: std::path::PathBuf,

    /// [optional] Output file path for the metadata JSON result.
    /// If not specified, defaults to <data_dir>/cryptpilot-verity.metadata.fb
    #[arg(short, long)]
    pub metadata: Option<std::path::PathBuf>,

    /// Output file path for the root hash ("-" for stdout)
    #[arg(long)]
    pub hash_output: std::path::PathBuf,

    /// Overwrite existing metadata file if it already exists.
    /// Intended for re-formatting or third-party auditing of an already formatted directory.
    #[arg(long)]
    pub force: bool,

    /// Label in key=value format. Can be specified multiple times.
    #[arg(long = "label", value_parser = parse_label)]
    pub labels: Vec<(String, String)>,
}

#[derive(Parser, Debug)]
pub struct VerifyOptions {
    /// Path to the data directory to verify
    #[arg()]
    pub data_dir: std::path::PathBuf,

    /// Expected root hash for verification
    #[arg()]
    pub hash: String,

    /// [optional] Path to the metadata file.
    /// If not specified, defaults to <data_dir>/cryptpilot-verity.metadata.fb
    #[arg(short, long)]
    pub metadata: Option<std::path::PathBuf>,

    /// Only verify metadata integrity without reading actual files.
    /// When enabled, only checks that the metadata hash matches the expected root hash,
    /// without verifying individual file contents against their descriptors
    #[arg(long, default_value = "false")]
    pub metadata_only: bool,
}

#[derive(Parser, Debug)]
pub struct DumpOptions {
    /// Path to the data directory from which to read metadata.
    /// Either data_dir or --metadata must be specified (not both required).
    /// If data_dir is provided without --metadata, reads from <data_dir>/cryptpilot-verity.metadata.fb
    #[arg(required_unless_present = "metadata")]
    pub data_dir: Option<std::path::PathBuf>,

    /// [optional] Path to the metadata file to read directly.
    /// Either --metadata or data_dir must be specified (not both required)
    #[arg(long, required_unless_present = "data_dir")]
    pub metadata: Option<std::path::PathBuf>,

    /// Print full metadata
    #[arg(long, required_unless_present_any = ["print_root_hash", "print_label"])]
    pub print_metadata: bool,

    /// Print only the root hash instead of full metadata
    #[arg(long, required_unless_present_any = ["print_metadata", "print_label"])]
    pub print_root_hash: bool,

    /// Print the value of a specific label key
    #[arg(long)]
    pub print_label: Option<String>,
}

#[derive(Parser, Debug)]
pub struct OpenOptions {
    /// Path to the data directory to mount
    #[arg()]
    pub data_dir: std::path::PathBuf,

    /// Mount point where the verity-fuse filesystem will be mounted
    #[arg()]
    pub mount_point: std::path::PathBuf,

    /// Expected root hash for verification
    #[arg()]
    pub hash: String,

    /// [optional] Path to the metadata file.
    /// If not specified, defaults to <data_dir>/cryptpilot-verity.metadata.fb
    #[arg(short, long)]
    pub metadata: Option<std::path::PathBuf>,

    /// Maximum number of verified data blocks to cache in memory.
    /// Each block is 4KB, so the default of 4096 caches up to 16MB.
    /// Larger values reduce disk I/O at the cost of more memory usage.
    #[arg(long, default_value_t = 4096)]
    pub block_cache_capacity: usize,
}

#[derive(Parser, Debug)]
pub struct CloseOptions {
    /// Mount point to unmount
    #[arg()]
    pub mount_point: std::path::PathBuf,
}
