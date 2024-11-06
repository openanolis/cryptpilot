pub mod global;
pub mod volume;

use std::path::{Path, PathBuf};

use lazy_static::lazy_static;
use tokio::sync::RwLock;

const CRYPTPILOT_CONFIG_DIR_DEFAULT: &'static str = "/etc/cryptpilot";

lazy_static! {
    static ref CRYPTPILOT_CONFIG_DIR: RwLock<PathBuf> =
        RwLock::new(CRYPTPILOT_CONFIG_DIR_DEFAULT.into());
}

pub async fn set_config_dir(config_dir: impl AsRef<Path>) {
    *(CRYPTPILOT_CONFIG_DIR.write().await) = PathBuf::from(config_dir.as_ref());
}

pub async fn get_config_dir() -> PathBuf {
    CRYPTPILOT_CONFIG_DIR.read().await.clone()
}
