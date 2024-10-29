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
    #[command(name = "crypttab-gen")]
    CrypttabGen(CrypttabGenOptions),

    #[command(name = "show")]
    Show(ShowOptions),

    #[command(name = "crypttab-key-supplier")]
    CrypttabKeySupplier(CrypttabKeySupplierOptions),
}

#[derive(Parser, Debug)]
pub struct CrypttabGenOptions {
    #[clap(long)]
    /// The Unix Domain Socket to listen on
    pub socket: Option<String>,
}

#[derive(Parser, Debug)]
pub struct ShowOptions {}

#[derive(Parser, Debug)]
pub struct CrypttabKeySupplierOptions {
    #[clap(long)]
    /// The Unix Domain Socket to listen on
    pub socket: Option<String>,
}
