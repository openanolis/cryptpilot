use anyhow::{bail, Context};
use clap::Parser;
use fuser::MountOption;
use tracing::info;
use verity_fuse::{cli::Cli, filesystem::VerityFS};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    if !cli.source.exists() {
        bail!("Source path does not exist: {:?}", cli.source);
    }
    if !cli.source.is_dir() {
        bail!("Source path is not a directory: {:?}", cli.source);
    }

    if !cli.mount_point.exists() {
        bail!("Mount point does not exist: {:?}", cli.mount_point)
    }

    let fs = VerityFS::new(&cli.source).context("Failed to create verity-fuse fs")?;

    info!(
        source = ?cli.source,
        mount_point = ?cli.mount_point,
        "Starting verity-fuse with recursive inode mapping"
    );

    fuser::mount2(
        fs,
        &cli.mount_point,
        &[
            MountOption::RO,
            MountOption::FSName("verity-fuse".into()),
            MountOption::AllowOther,
            MountOption::NoAtime, // Reduce noise
        ],
    )?;

    info!("Exited successfully.");

    Ok(())
}
