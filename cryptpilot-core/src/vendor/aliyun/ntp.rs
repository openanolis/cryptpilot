use anyhow::Result;
use rsntp::AsyncSntpClient;

use std::time::Duration;

pub async fn get_time_from_ntp() -> Result<Duration> {
    // Use the ntp server here: https://help.aliyun.com/zh/ecs/user-guide/alibaba-cloud-ntp-server#1d2319ae414lc
    let mut client = AsyncSntpClient::new();
    client.set_timeout(Duration::from_secs(10));
    let result = client.synchronize("ntp.cloud.aliyuncs.com").await?;
    let unix_timetamp_utc = result.datetime().unix_timestamp()?;
    Ok(unix_timetamp_utc)
}
