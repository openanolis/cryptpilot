use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    #[clap(long, short = 'd')]
    /// Path to the root directory where to load configuration files. Default value is /etc/cryptpilot.
    pub config_dir: Option<String>,
}

#[derive(Parser, Debug)]
pub enum Command {
    /// Show status about volumes.
    #[command(name = "show")]
    Show(ShowOptions),

    /// Initialize a new volume.
    #[command(name = "init")]
    Init(InitOptions),

    /// Open an existing volume.
    #[command(name = "open")]
    Open(OpenOptions),

    /// Close an open volume.
    #[command(name = "close")]
    Close(CloseOptions),

    /// Run as a systemd service.
    #[command(name = "systemd-service")]
    SystemdService(SystemdServiceOptions),
}

#[derive(Parser, Debug)]
pub struct ShowOptions {}

#[derive(Parser, Debug)]
pub struct InitOptions {
    /// Name of the volume to initialize.
    pub volume: String,

    /// Force re-initialization of the volume.
    #[clap(long, default_value = "false")]
    pub force_reinit: bool,

    /// Skip confirmation prompts.
    #[clap(long, short = 'y', default_value = "false")]
    pub yes: bool,
}

#[derive(Parser, Debug)]
pub struct OpenOptions {
    /// Name of the volume to open.
    pub volume: String,
}

#[derive(Parser, Debug)]
pub struct CloseOptions {
    /// Name of the volume to close.
    pub volume: String,
}

#[derive(Parser, Debug)]
pub struct SystemdServiceOptions {}
