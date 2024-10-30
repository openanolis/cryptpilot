pub(crate) mod cli;
pub(crate) mod cmd;
pub(crate) mod config;
pub(crate) mod luks2;
pub(crate) mod provider;
pub(crate) mod types;

use anyhow::Result;
use clap::Parser as _;
use log::debug;

#[tokio::main]
async fn main() -> Result<()> {
    let env = env_logger::Env::default()
        .default_filter_or("info")
        .default_write_style_or("always"); // enable color
    env_logger::Builder::from_env(env).init();

    let args = cli::Args::parse();

    if let Some(config_dir) = args.config_dir {
        config::set_config_dir(config_dir).await;
    }

    debug!("Using config dir: {:?}", config::get_config_dir().await);

    match args.command {
        cli::Command::Show(_) => cmd::show::cmd_show().await?,
        cli::Command::Init(init_options) => cmd::init::cmd_init(&init_options).await?,
        cli::Command::Open(open_options) => cmd::open::cmd_open(&open_options).await?,
        cli::Command::Close(close_options) => cmd::close::cmd_close(&close_options).await?,
    };

    Ok(())
}
