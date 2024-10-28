use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[clap(long, short)]
    pub config_dir: Option<String>,
}

#[derive(Parser, Debug)]
pub enum Command {
    #[command(name = "gen-crypttab")]
    GenCrypttab(GenCrypttabOptions),

    #[command(name = "show")]
    Show(ShowOptions),
}

#[derive(Parser, Debug)]
pub struct GenCrypttabOptions {}

#[derive(Parser, Debug)]
pub struct ShowOptions {}
