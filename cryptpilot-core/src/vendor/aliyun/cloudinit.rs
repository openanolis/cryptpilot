use anyhow::{Context as _, Result};
use std::time::Duration;

pub async fn get_aliyun_ecs_cloudinit_user_data() -> Result<String> {
    // Get cloud-init user data from IMDS: https://help.aliyun.com/zh/ecs/user-guide/view-instance-metadata
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let token = client
        .put("http://100.100.100.200/latest/api/token")
        .header("X-aliyun-ecs-metadata-token-ttl-seconds", "180")
        .send()
        .await
        .context("Failed to get IMDS token")?
        .text()
        .await?;

    let user_data = client
        .get("http://100.100.100.200/latest/user-data")
        .header("X-aliyun-ecs-metadata-token", token)
        .send()
        .await
        .context("Failed to get cloud-init user data from IMDS")?
        .text()
        .await?;

    Ok(user_data)
}
