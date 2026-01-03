use anyhow::{Context, Result};

pub async fn sync_time_to_system() -> Result<()> {
    let is_ecs = crate::vendor::aliyun::check_is_aliyun_ecs().await;
    if !is_ecs {
        tracing::debug!("Not a Aliyun ECS instance, skip syncing system time");
        return Ok(());
    } else {
        tracing::info!("Aliyun ECS instance detected, sync system time now");

        let unix_timetamp_utc = crate::vendor::aliyun::ntp::get_time_from_ntp()
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

    use std::time::Duration;

    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[tokio::test(flavor = "multi_thread", worker_threads = 10)]
    async fn test_time() -> Result<()> {
        let is_ecs = crate::vendor::aliyun::check_is_aliyun_ecs().await;
        if !is_ecs {
            /* Skip */
            return Ok(());
        } else {
            let unix_timetamp_utc = crate::vendor::aliyun::ntp::get_time_from_ntp()
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
