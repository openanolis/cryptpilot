pub(crate) mod cli;
pub(crate) mod config;
pub(crate) mod gen_crypttab;
pub(crate) mod provider;
pub(crate) mod show;

use anyhow::Result;
use clap::Parser as _;
use log::debug;

fn main() -> Result<()> {
    let env = env_logger::Env::default().default_write_style_or("always"); // enable color
    env_logger::Builder::from_env(env).init();

    let args = cli::Args::parse();

    if let Some(config_dir) = args.config_dir {
        config::set_config_dir(config_dir)?;
    }

    debug!("Using config dir: {:?}", config::get_config_dir()?);

    match args.command {
        cli::Command::GenCrypttab(_) => gen_crypttab::cmd_gen_crypttab()?,
        cli::Command::Show(_) => show::cmd_show()?,
    };

    Ok(())
}
