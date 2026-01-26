use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use cryptpilot::config::encrypt::EncryptConfig;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct FdeConfig {
    /// Configuration related to the read-only root filesystem.
    pub rootfs: RootFsConfig,

    /// Configuration related to the data partition.
    pub data: DataConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct RootFsConfig {
    /// The type of read-write overlay layer over the underhood read-only rootfs. Can be "disk", "disk-persist", or "ram". Default value is "disk".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rw_overlay: Option<RwOverlayType>,

    /// Encryption configuration for root filesystem. If not set, the rootfs partition WOULD NOT be encrypted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt: Option<EncryptConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    /// Whether or not to enable integrity check.
    #[serde(default = "Default::default")]
    pub integrity: bool,

    /// Encryption configuration for data partition. If not set, the data partition WOULD NOT be encrypted.
    pub encrypt: EncryptConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
#[serde(deny_unknown_fields)]
pub enum RwOverlayType {
    /// The overlay will be placed on disk but will be cleared on every boot.
    /// This is the default and recommended option for security.
    #[default]
    #[serde(rename = "disk")]
    Disk,
    /// The overlay will be placed on disk, and be persistent across reboots.
    /// Note: persistence depends on the data volume configuration.
    #[serde(rename = "disk-persist")]
    DiskPersist,
    /// The overlay will be placed on tmpfs (in RAM), and be cleared on reboot.
    #[serde(rename = "ram")]
    Ram,
}

#[cfg(test)]
pub mod tests {

    use cryptpilot::{
        config::encrypt::KeyProviderConfig,
        provider::kbs::{CdhType, KbsConfig},
    };

    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_deserialize() -> Result<()> {
        let raw = r#""#;
        assert!(toml::from_str::<FdeConfig>(raw).is_err());

        let raw = r#"
[rootfs]
rw_overlay = "disk"

[rootfs.encrypt.kbs]
kbs_url = "https://1.2.3.4:8080"
key_uri = "kbs:///default/test/rootfs_partition"
kbs_root_cert = """
-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"""

[data]
integrity = true

[data.encrypt.kbs]
kbs_url = "https://1.2.3.4:8080"
key_uri = "kbs:///default/test/data_partition"
kbs_root_cert = """
-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"""
    "#;
        let config: FdeConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            FdeConfig {
                rootfs: RootFsConfig {
                    rw_overlay: Some(RwOverlayType::Disk),
                    encrypt: Some(EncryptConfig {
                        key_provider: KeyProviderConfig::Kbs(KbsConfig {
                            cdh_type: CdhType::OneShot {
                                kbs_url: "https://1.2.3.4:8080".into(),
                                kbs_root_cert: Some(
                                    r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                                    .into()
                                ),
                            },
                            key_uri: "kbs:///default/test/rootfs_partition".into(),
                        })
                    })
                },
                data: DataConfig {
                    integrity: true,
                    encrypt: EncryptConfig {
                        key_provider: KeyProviderConfig::Kbs(KbsConfig {
                            cdh_type: CdhType::OneShot {
                                kbs_url: "https://1.2.3.4:8080".into(),
                                kbs_root_cert: Some(
                                    r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                                    .into()
                                ),
                            },
                            key_uri: "kbs:///default/test/data_partition".into(),
                        })
                    }
                }
            }
        );

        Ok(())
    }
}
