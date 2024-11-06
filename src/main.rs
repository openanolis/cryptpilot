pub(crate) mod cli;
pub(crate) mod cmd;
pub(crate) mod config;
pub(crate) mod luks2;
pub(crate) mod provider;
pub(crate) mod types;

use anyhow::Result;
use clap::Parser as _;
use log::debug;
use shadow_rs::shadow;

shadow!(build);

#[tokio::main]
async fn main() -> Result<()> {
    let args = cli::Args::parse();

    // Set config dir path
    if let Some(config_dir) = args.config_dir {
        config::set_config_dir(config_dir).await;
    }

    // Check verbose option from config file, if is running as systemd-service.
    let mut log_level = "info";
    if let cli::Command::SystemdService(_) = args.command {
        let global_config = crate::config::global::get_global_config().await?;
        if global_config.systemd.verbose {
            log_level = "debug";
            crate::luks2::set_verbose(true).await;
        }
    }

    // Config env_logger
    let env = env_logger::Env::default()
        .default_filter_or(log_level)
        .default_write_style_or("always"); // enable color
    env_logger::Builder::from_env(env).init();

    debug!("Using config dir: {:?}", config::get_config_dir().await);

    match args.command {
        cli::Command::Show(_) => cmd::show::cmd_show().await?,
        cli::Command::Init(init_options) => cmd::init::cmd_init(&init_options).await?,
        cli::Command::Open(open_options) => cmd::open::cmd_open(&open_options).await?,
        cli::Command::Close(close_options) => cmd::close::cmd_close(&close_options).await?,
        cli::Command::SystemdService(systemd_service_options) => {
            cmd::systemd_service::cmd_systemd_service(&systemd_service_options).await?
        }
    };

    Ok(())
}
