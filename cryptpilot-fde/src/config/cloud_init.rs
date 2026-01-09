use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;

use crate::config::FdeConfigBundle;

pub const CLOUD_INIT_FDE_CONFIG_BUNDLE_HEADER: &str = "#cryptpilot-fde-config";

/// This is a config source that loads config from aliyun cloud-init user data. It is only supported in aliyun ECS instance, with IMDS enabled.
/// User is expected to put the config bundle in the user data of the instance, before the instance boots.
///
/// The config bundle is a TOML string, with special header string [CLOUD_INIT_CONFIG_BUNDLE_HEADER].
///
/// To configure the user data, please refer to this link: https://help.aliyun.com/zh/ecs/user-guide/customize-the-initialization-configuration-for-an-instance
pub struct CloudInitConfigSource {}

impl Default for CloudInitConfigSource {
    fn default() -> Self {
        Self::new()
    }
}

impl CloudInitConfigSource {
    pub fn new() -> Self {
        Self {}
    }

    fn parse_from_user_data(user_data: &str) -> Result<FdeConfigBundle> {
        if user_data.trim().is_empty() {
            bail!("The cloud-init user data is empty")
        }

        if !user_data.starts_with(CLOUD_INIT_FDE_CONFIG_BUNDLE_HEADER) {
            bail!(
                "Cannot find cryptplot header in cloud-init user data, maybe it is not cryptpilot config bundle"
            )
        }

        let fde_config_bundle: FdeConfigBundle =
            toml::from_str(user_data).context("Failed to parse cloud-init user data")?;

        Ok(fde_config_bundle)
    }
}

#[async_trait]
impl super::FdeConfigSource for CloudInitConfigSource {
    fn source_debug_string(&self) -> String {
        "aliyun cloud-init user data".into()
    }

    async fn get_fde_config_bundle(&self) -> Result<FdeConfigBundle> {
        let is_ecs = cryptpilot::vendor::aliyun::check_is_aliyun_ecs().await;
        if !is_ecs {
            bail!("Not a Aliyun ECS instance, skip fetching config from cloud-init user data");
        } else {
            let user_data =
                cryptpilot::vendor::aliyun::cloudinit::get_aliyun_ecs_cloudinit_user_data().await?;
            Self::parse_from_user_data(&user_data)
        }
    }
}

#[cfg(test)]
pub mod tests {
    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_deserialize_from_initrd() -> Result<()> {
        CloudInitConfigSource::parse_from_user_data(
            r#"#cryptpilot-fde-config

[global.boot]
verbose = true

[fde.rootfs]
rw_overlay = "disk"

[fde.rootfs.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]

[fde.data]
integrity = true

[fde.data.encrypt.exec]
command = "echo"
args = ["-n", "AAAaaawewe222"]"#,
        )?;

        Ok(())
    }
}
