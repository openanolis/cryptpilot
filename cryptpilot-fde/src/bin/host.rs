use std::path::Path;

use anyhow::{bail, Result};
use clap::Parser as _;
use cryptpilot_fde::cli::Cli;
use cryptpilot_fde::cmd::IntoCommand;
use cryptpilot_fde::config::{
    cached::CachedFdeConfigSource, fs::FileSystemConfigSource,
    initrd_state::InitrdStateConfigSource,
};
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

shadow!(build);

#[tokio::main]
async fn main() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    let (filter, _reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    cryptpilot::fs::set_verbose(
        tracing::enabled!(target: "cryptpilot-fde-host", tracing::Level::DEBUG),
    )
    .await;

    let args = Cli::parse();

    if args.config_dir.is_some() {
        bail!("Cannot specify `--config-dir` with `show-reference-value` or `config` subcommand");
    }

    if Path::new("/etc/initrd-release").exists() {
        // If we are in initrd emergency shell, try loading from initrd state first,
        // otherwise fall back to filesystem config.
        if InitrdStateConfigSource::exist() {
            cryptpilot_fde::config::set_fde_config_source(CachedFdeConfigSource::new(
                InitrdStateConfigSource::new(),
            ))
            .await;
        } else {
            let fs_source = FileSystemConfigSource::new_with_default_config_dir();
            cryptpilot_fde::config::set_fde_config_source(CachedFdeConfigSource::new(fs_source))
                .await;
        }
    }

    tracing::debug!(
        "Using config source from {:?}",
        cryptpilot_fde::config::get_fde_config_source()
            .await
            .source_debug_string()
    );

    args.command.into_command().run().await?;

    Ok(())
}
