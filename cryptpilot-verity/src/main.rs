use anyhow::Result;
use clap::Parser as _;
use shadow_rs::shadow;
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _};

use crate::cmd::IntoCommand as _;

mod cli;
mod cmd;

shadow!(build);

#[tokio::main]
async fn main() -> Result<()> {
    let filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    let args = cli::Cli::parse();

    // Handle the command
    args.command.into_command().run().await?;

    Ok(())
}
