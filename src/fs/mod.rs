pub mod block;
pub mod cmd;
pub mod luks2;
pub mod mkfs;
pub mod mount;
pub mod shell;

use lazy_static::lazy_static;
use tokio::sync::RwLock;

lazy_static! {
    static ref VERBOSE: RwLock<bool> = RwLock::new(false);
}

pub async fn set_verbose(verbose: bool) {
    *VERBOSE.write().await = verbose;
}

async fn get_verbose() -> bool {
    *VERBOSE.read().await
}
