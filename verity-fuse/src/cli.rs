use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
pub struct Cli {
    #[arg(short, long)]
    pub source: PathBuf,
    #[arg(short, long)]
    pub mount_point: PathBuf,
}