use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use cryptpilot::config::encrypt::EncryptConfig;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct FdeConfig {
    /// Configuration related to the read-only root filesystem.
    pub rootfs: RootFsConfig,

    /// Configuration related to the writeable delta volume on disk.
    #[serde(alias = "data")]
    pub delta: DeltaConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct RootFsConfig {
    /// The locaton for storing the delta data over the underhood read-only rootfs. Can be "disk", "disk-persist", or "ram". Default value is "disk".
    #[serde(skip_serializing_if = "Option::is_none", alias = "rw_overlay")]
    pub delta_location: Option<DeltaLocation>,

    /// The backend implementation for the delta data layer. Can be "overlayfs" or "dm-snapshot". Default value is "dm-snapshot".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_backend: Option<DeltaBackend>,

    /// Encryption configuration for root filesystem. If not set, the rootfs partition WOULD NOT be encrypted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypt: Option<EncryptConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct DeltaConfig {
    /// Whether or not to enable integrity check.
    #[serde(default = "Default::default")]
    pub integrity: bool,

    /// Encryption configuration for delta partition. If not set, the delta partition WOULD NOT be encrypted.
    pub encrypt: EncryptConfig,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone, Default)]
#[serde(deny_unknown_fields)]
pub enum DeltaLocation {
    /// The delta data will be placed on disk but will be cleared on every boot.
    /// This is the default and recommended option for security.
    #[default]
    #[serde(rename = "disk")]
    Disk,
    /// The delta data will be placed on disk, and be persistent across reboots.
    /// Note: persistence depends on the delta volume configuration.
    #[serde(rename = "disk-persist")]
    DiskPersist,
    /// The delta data will be placed on tmpfs (in RAM), and be cleared on reboot.
    #[serde(rename = "ram")]
    Ram,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Copy, Clone, Default)]
#[serde(deny_unknown_fields)]
pub enum DeltaBackend {
    /// Use overlayfs for file-level copy-on-write.
    #[serde(rename = "overlayfs")]
    Overlayfs,
    /// Use dm-snapshot for block-level copy-on-write. This is the default.
    #[default]
    #[serde(rename = "dm-snapshot")]
    DmSnapshot,
}

#[cfg(test)]
mod tests {

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
delta_location = "disk"
delta_backend = "dm-snapshot"

[rootfs.encrypt.kbs]
kbs_url = "https://1.2.3.4:8080"
key_uri = "kbs:///default/test/rootfs_partition"
kbs_root_cert = """
-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"""

[delta]
integrity = true

[delta.encrypt.kbs]
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
                    delta_location: Some(DeltaLocation::Disk),
                    delta_backend: Some(DeltaBackend::DmSnapshot),
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
                delta: DeltaConfig {
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
