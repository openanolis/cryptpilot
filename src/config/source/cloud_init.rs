use anyhow::{bail, Context as _, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::config::{fde::FdeConfig, global::GlobalConfig, ConfigBundle};

use super::ConfigSource;

pub const CLOUD_INIT_FDE_CONFIG_BUNDLE_HEADER: &str = "#cryptpilot-fde-config";

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(deny_unknown_fields)]
pub struct FdeConfigBundle {
    /// Global configuration. This is the same as the `/etc/cryptpilot/global.toml` file.
    pub global: Option<GlobalConfig>,

    /// Configuration related to full disk encryption (FDE). This is the same as the `/etc/cryptpilot/fde.toml` file.
    pub fde: Option<FdeConfig>,
}

impl FdeConfigBundle {
    pub fn gen_hash_content(&self) -> Result<String> {
        Ok(toml::to_string(&self)?)
    }

    pub fn gen_hash_content_pretty(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self)?)
    }

    pub fn gen_hash_hex(&self) -> Result<String> {
        let content_to_hash = self.gen_hash_content()?;
        let hash = sha2::Sha384::new()
            .chain_update(content_to_hash)
            .finalize()
            .to_vec();
        let hash_hex = hex::encode(hash);

        Ok(hash_hex)
    }

    pub fn flatten_to_config_bundle(self) -> ConfigBundle {
        ConfigBundle {
            global: self.global,
            fde: self.fde,
            volumes: vec![],
        }
    }
}

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

    fn parse_from_user_data(user_data: &str) -> Result<ConfigBundle> {
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

        let config_bundle = fde_config_bundle.flatten_to_config_bundle();

        Ok(config_bundle)
    }
}

#[async_trait]
impl ConfigSource for CloudInitConfigSource {
    fn source_debug_string(&self) -> String {
        "aliyun cloud-init user data".into()
    }

    async fn get_config(&self) -> Result<ConfigBundle> {
        let is_ecs = crate::vendor::aliyun::check_is_aliyun_ecs().await;
        if !is_ecs {
            bail!("Not a Aliyun ECS instance, skip fetching config from cloud-init user data");
        } else {
            let user_data =
                crate::vendor::aliyun::cloudinit::get_aliyun_ecs_cloudinit_user_data().await?;
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
