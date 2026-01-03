use std::path::PathBuf;

const ONE_SHOT_CDH_BINARY_PATH: &str = "/usr/bin/confidential-data-hub";

pub fn find_cdh_binary_or_default() -> PathBuf {
    which::which("confidential-data-hub").unwrap_or(ONE_SHOT_CDH_BINARY_PATH.into())
}
