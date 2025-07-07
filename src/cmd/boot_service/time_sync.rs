use anyhow::{Context, Result};
use rsntp::AsyncSntpClient;
use tokio::net::TcpStream;

use std::time::Duration;

async fn check_is_aliyun_ecs() -> bool {
    matches!(
        tokio::time::timeout(
            Duration::from_secs(5),
            TcpStream::connect(("100.100.100.200", 80)),
        )
        .await,
        Ok(Ok(_))
    )
}

async fn get_time_from_ntp() -> Result<Duration> {
    // Use the ntp server here: https://help.aliyun.com/zh/ecs/user-guide/alibaba-cloud-ntp-server#1d2319ae414lc
    let mut client = AsyncSntpClient::new();
    client.set_timeout(Duration::from_secs(10));
    let result = client.synchronize("ntp.cloud.aliyuncs.com").await?;
    let unix_timetamp_utc = result.datetime().unix_timestamp()?;
    Ok(unix_timetamp_utc)
}

pub async fn sync_time_to_system() -> Result<()> {
    let is_ecs = check_is_aliyun_ecs().await;
    if !is_ecs {
        tracing::debug!("Not a Aliyun ECS instance, skip syncing system time");
        return Ok(());
    } else {
        tracing::info!("Aliyun ECS instance detected, sync system time now");

        let unix_timetamp_utc = get_time_from_ntp()
            .await
            .context("Failed to get time from NTP server")?;
        tracing::info!(?unix_timetamp_utc, "Got UNIX timestamp from NTP server");

        // Note CAP_SYS_TIME is required to set system time
        nix::time::clock_settime(
            nix::time::ClockId::CLOCK_REALTIME,
            nix::sys::time::TimeSpec::from_duration(unix_timetamp_utc),
        )
        .context("Failed to set system time")?;
    }

    Ok(())
}

#[cfg(test)]
pub mod tests {

    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_time() -> Result<()> {
        let is_ecs = check_is_aliyun_ecs().await;
        if !is_ecs {
            /* Skip */
            return Ok(());
        } else {
            let unix_timetamp_utc = get_time_from_ntp()
                .await
                .context("Failed to get time from NTP server")?;

            println!("Got: {unix_timetamp_utc:?}");
            let current_sys_time = Duration::from(nix::time::clock_gettime(
                nix::time::ClockId::CLOCK_REALTIME,
            )?);

            let diff = if current_sys_time >= unix_timetamp_utc {
                current_sys_time - unix_timetamp_utc
            } else {
                unix_timetamp_utc - current_sys_time
            };
            assert!(diff <= Duration::from_secs(10));

            Ok(())
        }
    }
}
