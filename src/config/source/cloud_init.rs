use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;

use crate::config::ConfigBundle;

use super::ConfigSource;

/// This is a config source that loads config from aliyun cloud-init user data. It is only supported in aliyun ECS instance, with IMDS enabled.
/// User is expected to put the config bundle in the user data of the instance, before the instance boots.
///
/// The config bundle is a TOML string, with special header string [CLOUD_INIT_CONFIG_BUNDLE_HEADER].
///
/// To configure the user data, please refer to this link: https://help.aliyun.com/zh/ecs/user-guide/customize-the-initialization-configuration-for-an-instance
pub struct CloudInitConfigSource {}

impl CloudInitConfigSource {
    pub fn new() -> Self {
        Self {}
    }
}

pub const CLOUD_INIT_CONFIG_BUNDLE_HEADER: &str = "#cryptpilot-config";

#[async_trait]
impl ConfigSource for CloudInitConfigSource {
    fn source_debug_string(&self) -> String {
        "aliyun cloud-init user data".into()
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        // Get cloud-init user data from IMDS: https://help.aliyun.com/zh/ecs/user-guide/view-instance-metadata
        let token = reqwest::Client::new()
            .put("http://100.100.100.200/latest/api/token")
            .header("X-aliyun-ecs-metadata-token-ttl-seconds", "180")
            .send()
            .await
            .context("Failed to get IMDS token")?
            .text()
            .await?;

        let user_data = reqwest::Client::new()
            .get("http://100.100.100.200/latest/user-data")
            .header("X-aliyun-ecs-metadata-token", token)
            .send()
            .await
            .context("Failed to get cloud-init user data from IMDS")?
            .text()
            .await?;

        if user_data.trim().is_empty() {
            bail!("The cloud-init user data is empty")
        }

        if !user_data.starts_with(CLOUD_INIT_CONFIG_BUNDLE_HEADER) {
            bail!(
                "Cannot find cryptplot header in cloud-init user data, maybe it is not cryptpilot config bundle"
            )
        }

        let config_bundle: ConfigBundle =
            toml::from_str(&user_data).context("Failed to parse cloud-init user data")?;

        Ok(config_bundle)
    }
}
