use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[clap(long, short = 'd')]
    pub config_dir: Option<String>,
}

#[derive(Parser, Debug)]
pub enum Command {
    #[command(name = "show")]
    Show(ShowOptions),

    #[command(name = "init")]
    Init(InitOptions),

    #[command(name = "open")]
    Open(OpenOptions),

    #[command(name = "close")]
    Close(CloseOptions),
}

#[derive(Parser, Debug)]
pub struct ShowOptions {}

#[derive(Parser, Debug)]
pub struct InitOptions {
    pub volume: String,
}

#[derive(Parser, Debug)]
pub struct OpenOptions {
    pub volume: String,
}

#[derive(Parser, Debug)]
pub struct CloseOptions {
    pub volume: String,
}
