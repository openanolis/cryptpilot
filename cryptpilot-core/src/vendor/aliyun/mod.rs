use std::time::Duration;

use anyhow::{bail, Context as _, Result};
use tokio::net::TcpStream;

pub mod cloudinit;
pub mod ntp;

/// Check whether the current host is an Aliyun ECS instance by probing the
/// IMDS endpoint (100.100.100.200:80).  Returns `Ok(())` when the endpoint is
/// reachable, or a descriptive error explaining *why* the check failed so that
/// callers can produce actionable log messages instead of the generic
/// "not an ECS instance" phrasing.
pub async fn check_is_aliyun_ecs() -> Result<()> {
    match tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect(("100.100.100.200", 80)),
    )
    .await
    {
        Err(_elapsed) => bail!(
            "Timed out connecting to Aliyun IMDS endpoint (100.100.100.200:80). \
             The instance may not be an Aliyun ECS, or IMDS may be disabled / \
             blocked by a security-group rule."
        ),
        Ok(Err(e)) => Err(e).context(
            "Failed to connect to Aliyun IMDS endpoint (100.100.100.200:80). \
             The instance may not be an Aliyun ECS, or IMDS may be disabled.",
        ),
        Ok(Ok(_)) => Ok(()),
    }
}
