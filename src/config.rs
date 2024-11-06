use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Context as _, Result};
use lazy_static::lazy_static;
use log::debug;
use run_script::ScriptOptions;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const CRYPTPILOT_CONFIG_DIR_DEFAULT: &'static str = "/etc/cryptpilot";

lazy_static! {
    static ref CRYPTPILOT_CONFIG_DIR: RwLock<PathBuf> =
        RwLock::new(CRYPTPILOT_CONFIG_DIR_DEFAULT.into());
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct VolumeConfig {
    /// The name of resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
    pub volume: String,

    /// The identifier of the underlying encrypted device.
    pub dev: String,

    #[serde(flatten)]
    pub extra_options: ExtraOptions,

    /// The key provider specific options
    pub key_provider: KeyProviderOptions,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ExtraOptions {
    /// Whether or not to open the LUKS2 device and set up mapping during system booting phase (the phase after initrd phase)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_in_system: Option<bool>,

    // pub open_in_initrd: Option<bool>,
    /// The file system to initialize on the volume. If is not specified, or the device is not "empty", i.e. it contains any signature, the operation will be skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub makefs: Option<MakeFsType>,

    /// Whether or not to enable support for data integrity. The default value is false. Note that integrity cannot prevent a replay (rollback) attack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum MakeFsType {
    Swap,
    Ext4,
    Xfs,
    Vfat,
}

impl MakeFsType {
    pub fn to_systemd_makefs_fstype(&self) -> &'static str {
        match self {
            MakeFsType::Swap => "swap",
            MakeFsType::Ext4 => "ext4",
            MakeFsType::Xfs => "xfs",
            MakeFsType::Vfat => "vfat",
        }
    }

    pub fn mkfs_on_no_wipe_volume_blocking(&self, volume_path: &str) -> Result<()> {
        let mut ops = ScriptOptions::new();
        ops.exit_on_error = true;
        match self {
            MakeFsType::Swap => {
                run_script::run_script!(
                    format!(
                        r#"
                        dd if=/dev/zero of={volume_path} count=1 seek=0 bs=4096
                        mkswap {volume_path}
                        "#,
                    ),
                    ops
                )
            }
            MakeFsType::Ext4 => {
                run_script::run_script!(
                    format!(
                        r#"
                        BLOCKS=$(mkfs.ext4 -n {volume_path} | tail -n 4 | grep -Eo '[0-9]{{4,}}' | sort -n)
                        BLOCKS="0 $BLOCKS"

                        for BLOCK_NUM in $BLOCKS
                        do
                            dd if=/dev/zero of={volume_path} count=1 seek=$BLOCK_NUM bs=4096
                            dd if={volume_path}  count=1 skip=$BLOCK_NUM  bs=4096 | hexdump 
                        done
                        mkfs.ext4 {volume_path}
                        "#,
                    ),
                    ops
                )
            }
            MakeFsType::Xfs => {
                run_script::run_script!(
                    format!(
                        r#"
                        dd if=/dev/zero of={volume_path} count=1 seek=0 bs=4096
                        mkfs.xfs -f {volume_path}
                        "#,
                    ),
                    ops
                )
            }
            MakeFsType::Vfat => {
                bail!("The option `makefs=vfat` and `integrity=true` is not currently supported")
            }
        }
        .map_err(Into::into)
        .and_then(|(code, output, error)| {
            if code != 0 {
                bail!("Bad exit code: {code}\n\tstdout: {output}\n\tstderr: {error}")
            } else {
                Ok(())
            }
        })
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum KeyProviderOptions {
    #[cfg(feature = "provider-otp")]
    Otp(crate::provider::otp::OtpOptions),
    #[cfg(feature = "provider-kms")]
    Kms(crate::provider::kms::KmsOptions),
    #[cfg(feature = "provider-kbs")]
    Kbs(crate::provider::kbs::KbsOptions),
    #[cfg(feature = "provider-tpm2")]
    Tpm2(crate::provider::tpm2::Tpm2Options),
}

pub async fn set_config_dir(config_dir: impl AsRef<Path>) {
    *(CRYPTPILOT_CONFIG_DIR.write().await) = PathBuf::from(config_dir.as_ref());
}

pub async fn get_config_dir() -> PathBuf {
    CRYPTPILOT_CONFIG_DIR.read().await.clone()
}

pub async fn load_volume_configs() -> Result<Vec<VolumeConfig>> {
    let mut volume_configs = Vec::new();
    let config_dir = get_config_dir().await.join("volumes");

    debug!("Loading volume configs from: {config_dir:?}");
    if !config_dir.exists() {
        bail!("Directory not found: {}", config_dir.display());
    }

    let mut volume_names = HashSet::<String>::new();

    let mut entries = tokio::fs::read_dir(config_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.is_file() && path.extension().map_or(false, |ext| ext == "toml") {
            let volume_config = tokio::fs::read_to_string(&path)
                .await
                .map_err(Into::into)
                .and_then(|content| {
                    toml::from_str::<VolumeConfig>(&content)
                        .context("Failed to parse content as TOML")
                })
                .and_then(|volume_config| {
                    if volume_names.contains(&volume_config.volume) {
                        bail!(
                            "Volume `{}` is already defined in other volume config files. Please checking your volume config files.",
                            volume_config.volume
                        )
                    }

                    volume_names.insert(volume_config.volume.to_owned());
                    Ok(volume_config)
                })
                .with_context(|| format!("Failed to loading volume config file: {}", path.display()))?;

            volume_configs.push(volume_config);
        }
    }

    volume_configs.sort_by(|a, b| a.volume.cmp(&b.volume));

    Ok(volume_configs)
}

pub async fn load_volume_config(volume: &str) -> Result<VolumeConfig> {
    crate::config::load_volume_configs()
        .await
        .and_then(|volume_configs| {
            let volume_config = volume_configs
                .into_iter()
                .find(|volume_config| volume_config.volume == volume)
                .ok_or_else(|| anyhow!("Unknown volume name: {volume}"))?;

            Ok(volume_config)
        })
        .with_context(|| format!("Failed to load config for volume name: {}", volume))
}

#[cfg(test)]
pub mod tests {

    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_deserialize_otp() -> Result<()> {
        let raw = r#"
        dev = "/dev/nvme1n1p1"
        volume = "data"

        [key_provider.otp]
        "#;

        let config: VolumeConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            VolumeConfig {
                volume: "data".into(),
                dev: "/dev/nvme1n1p1".into(),
                key_provider: KeyProviderOptions::Otp(crate::provider::otp::OtpOptions {}),
                extra_options: ExtraOptions {
                    open_in_system: None,
                    makefs: None,
                    integrity: None,
                }
            }
        );
        Ok(())
    }

    #[test]
    fn test_deserialize_kms() -> Result<()> {
        let raw = r#"
dev = "/dev/nvme1n1p2
"
volume = "data1"

[key_provider.kms]
secret_name = "luks_passphrase"
client_key = '''
{
  "KeyId": "KAAP.f4c8****",
  "PrivateKeyData": "MIIJ****"
}'''
client_key_password = "fa79****"
kms_instance_id = "kst-bjj66bdba95w1m0xfm3bt"
kms_cert_pem = """
-----BEGIN CERTIFICATE-----
MIIDuzCCAqOgAwIBAgIJALTKwWAjvbMiMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
ZSBLTVMgUm9vdCBDQTAeFw0yMTA3MDgwNjU1MjlaFw00MTA3MDMwNjU1MjlaMHQx
CzAJBgNVBAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1po
b3UxEDAOBgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwT
UHJpdmF0ZSBLTVMgUm9vdCBDQTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoC
ggEBAM99IVpxedcGYZVXXX4XZ+bYWw1gVD5Uli9kBrlq3nBT8c0b+4/1W4aQzr+S
zBEWMrRZaMH3c5rV63qILyy8w4Gm2J0++nIA7uXVhpbliq6lf6p0i3cKpp+JGCbP
kLvOpONrZ4an/htNE+vpfbsW3WcwcVbmZpQyuGIXIST8iyfTwckZSMkxAPW4rhMa
QtmQcQiWaJsR0WJoqP7jXcHZobYehnUlzi/ZzdtmnkhTjz0+GvX9/1GBHCyfVEOO
a0RiT5nEz55xWahZKbj+1nhmInbc7BUqfhz/mbQjtk5lAsJpA8JrbukRhTiAMbj9
TqUqLe/meEVdjtD6wWsaZoSeoucCAwEAAaNQME4wHQYDVR0OBBYEFAVKzUR5/d6j
nYM/bHlxURkGhe2EMB8GA1UdIwQYMBaAFAVKzUR5/d6jnYM/bHlxURkGhe2EMAwG
A1UdEwQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAMCxpkV/KPuKVOBsT4yYkeX1
Q7IQQoICOAkZOkg7KEej4BJpW2Ml121aFScKxdnRcV2omr48K+GQt/mPNAXgf3k2
eKQce7xmBwntRplDJFrzdZPBdjel4i62JoWlaTejht2L5ano+x3a3keqF0GoOnn0
StwpG2xa0S6RmyNFiMugwDBCtSCJAJKr8yAbO+hoe1lQR0M78dy8ENteC/BXuAks
cktoG0/ryX9EqE9xQ2Do3INDq2PxIuA9yPvZ1eV3xa3bd1u+02feGIrtc9cJ5chf
vUk5tbgg58NVXrg29yE5eq3j2BErUlAs2LB/Bt/Jhkekvp7qR42btJj+/zQnDSw=
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIID3zCCAsegAwIBAgIJAO8qnQyTy8/kMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
ZSBLTVMgUm9vdCBDQTAeFw0yMjAyMjIwNTAwMDZaFw00MjAyMTcwNTAwMDZaMIGB
MQswCQYDVQQGEwJDTjERMA8GA1UECAwIWmhlSmlhbmcxETAPBgNVBAcMCEhhbmda
aG91MRAwDgYDVQQKDAdBbGliYWJhMQ8wDQYDVQQLDAZBbGl5dW4xKTAnBgNVBAMM
IFByaXZhdGUgS01TIGNuLWJlaWppbmcgU2Vjb25kIENBMIIBIjANBgkqhkiG9w0B
AQEFAAOCAQ8AMIIBCgKCAQEAxjz6ltGz06I5BqSCabzxtvma20LcpHHKPqG3D/zb
OS5XaOa5WOawvZUQueIXoDFnH0a/53NzfTPW8ET/0/ls7z1deirSHUmi5gUDCrit
DdyO3bieJ0kMMinjdLGIe8hnd2H7v/X06tU+KilsEFAfFdKyVETa5iffHZRnWUlh
NfoKAU9ycuJ2NGRE0lQ7uSB1ekCHxTNd4rsf0Oqj2xQJfR1jthf/m6rjc38/RkEM
eI1YeADRDKxbDCmFciHs8B+q/pO+q3+o3rKhLXlu8vrJngG3tRsn/i1TQBXjAIdB
sA2RBcni75VqATFImD9TetjwK8+oi1mdBm2WylTPm/y30QIDAQABo2YwZDAdBgNV
HQ4EFgQUW0FY+K5NfCyUqgVjp5vH11aEUlwwHwYDVR0jBBgwFoAUBUrNRHn93qOd
gz9seXFRGQaF7YQwEgYDVR0TAQH/BAgwBgEB/wIBADAOBgNVHQ8BAf8EBAMCAYYw
DQYJKoZIhvcNAQELBQADggEBAI8dvj/5rTK4NxC6cNeRi4wF8HDLHLEVbOHfxQDr
99aQmLqDL6rc9LbzRqtH8Pga606J0NsB4owyEiumYjOUyPOVyUYKrxKt5Wj/0C3V
/sHKOdaRS+yT6O8XcsTddxbl9cIw6WroTRFvqnAtiaOt3JMCmU2rXjYa8w5tz/1t
gTwmDuN5u4+N+zfoK0Cc2hvMJdiYFhzPYbie1ffmcHXJTNPqUg9K2lfqDCmZ+xIA
PpVsaCU9401qPWRWftXJgb3vIVOsYB6l3KYYKdOpudaCzSbZVROmC4a693/E5hWM
nc8BTncWI0KGWIzTQasuSEye50R6gc9wZCGIElmhWcu3NYk=
-----END CERTIFICATE-----
"""
        "#;
        let config: VolumeConfig = toml::from_str(raw)?;

        let expected = VolumeConfig {
            volume: "data1".into(),
            dev: "/dev/nvme1n1p2
            "
            .into(),
            extra_options: ExtraOptions {
                open_in_system: None,
                makefs: None,
                integrity: None,
            },
            key_provider: KeyProviderOptions::Kms(crate::provider::kms::KmsOptions {
                client_key: r#"{
  "KeyId": "KAAP.f4c8****",
  "PrivateKeyData": "MIIJ****"
}"#
                .to_owned(),
                client_key_password: "fa79****".to_owned(),
                kms_instance_id: "kst-bjj66bdba95w1m0xfm3bt".to_owned(),
                kms_cert_pem: r#"-----BEGIN CERTIFICATE-----
MIIDuzCCAqOgAwIBAgIJALTKwWAjvbMiMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
ZSBLTVMgUm9vdCBDQTAeFw0yMTA3MDgwNjU1MjlaFw00MTA3MDMwNjU1MjlaMHQx
CzAJBgNVBAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1po
b3UxEDAOBgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwT
UHJpdmF0ZSBLTVMgUm9vdCBDQTCCASIwDQYJKoZIhvcNAQEBBQADggEPADCCAQoC
ggEBAM99IVpxedcGYZVXXX4XZ+bYWw1gVD5Uli9kBrlq3nBT8c0b+4/1W4aQzr+S
zBEWMrRZaMH3c5rV63qILyy8w4Gm2J0++nIA7uXVhpbliq6lf6p0i3cKpp+JGCbP
kLvOpONrZ4an/htNE+vpfbsW3WcwcVbmZpQyuGIXIST8iyfTwckZSMkxAPW4rhMa
QtmQcQiWaJsR0WJoqP7jXcHZobYehnUlzi/ZzdtmnkhTjz0+GvX9/1GBHCyfVEOO
a0RiT5nEz55xWahZKbj+1nhmInbc7BUqfhz/mbQjtk5lAsJpA8JrbukRhTiAMbj9
TqUqLe/meEVdjtD6wWsaZoSeoucCAwEAAaNQME4wHQYDVR0OBBYEFAVKzUR5/d6j
nYM/bHlxURkGhe2EMB8GA1UdIwQYMBaAFAVKzUR5/d6jnYM/bHlxURkGhe2EMAwG
A1UdEwQFMAMBAf8wDQYJKoZIhvcNAQELBQADggEBAMCxpkV/KPuKVOBsT4yYkeX1
Q7IQQoICOAkZOkg7KEej4BJpW2Ml121aFScKxdnRcV2omr48K+GQt/mPNAXgf3k2
eKQce7xmBwntRplDJFrzdZPBdjel4i62JoWlaTejht2L5ano+x3a3keqF0GoOnn0
StwpG2xa0S6RmyNFiMugwDBCtSCJAJKr8yAbO+hoe1lQR0M78dy8ENteC/BXuAks
cktoG0/ryX9EqE9xQ2Do3INDq2PxIuA9yPvZ1eV3xa3bd1u+02feGIrtc9cJ5chf
vUk5tbgg58NVXrg29yE5eq3j2BErUlAs2LB/Bt/Jhkekvp7qR42btJj+/zQnDSw=
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIID3zCCAsegAwIBAgIJAO8qnQyTy8/kMA0GCSqGSIb3DQEBCwUAMHQxCzAJBgNV
BAYTAkNOMREwDwYDVQQIDAhaaGVKaWFuZzERMA8GA1UEBwwISGFuZ1pob3UxEDAO
BgNVBAoMB0FsaWJhYmExDzANBgNVBAsMBkFsaXl1bjEcMBoGA1UEAwwTUHJpdmF0
ZSBLTVMgUm9vdCBDQTAeFw0yMjAyMjIwNTAwMDZaFw00MjAyMTcwNTAwMDZaMIGB
MQswCQYDVQQGEwJDTjERMA8GA1UECAwIWmhlSmlhbmcxETAPBgNVBAcMCEhhbmda
aG91MRAwDgYDVQQKDAdBbGliYWJhMQ8wDQYDVQQLDAZBbGl5dW4xKTAnBgNVBAMM
IFByaXZhdGUgS01TIGNuLWJlaWppbmcgU2Vjb25kIENBMIIBIjANBgkqhkiG9w0B
AQEFAAOCAQ8AMIIBCgKCAQEAxjz6ltGz06I5BqSCabzxtvma20LcpHHKPqG3D/zb
OS5XaOa5WOawvZUQueIXoDFnH0a/53NzfTPW8ET/0/ls7z1deirSHUmi5gUDCrit
DdyO3bieJ0kMMinjdLGIe8hnd2H7v/X06tU+KilsEFAfFdKyVETa5iffHZRnWUlh
NfoKAU9ycuJ2NGRE0lQ7uSB1ekCHxTNd4rsf0Oqj2xQJfR1jthf/m6rjc38/RkEM
eI1YeADRDKxbDCmFciHs8B+q/pO+q3+o3rKhLXlu8vrJngG3tRsn/i1TQBXjAIdB
sA2RBcni75VqATFImD9TetjwK8+oi1mdBm2WylTPm/y30QIDAQABo2YwZDAdBgNV
HQ4EFgQUW0FY+K5NfCyUqgVjp5vH11aEUlwwHwYDVR0jBBgwFoAUBUrNRHn93qOd
gz9seXFRGQaF7YQwEgYDVR0TAQH/BAgwBgEB/wIBADAOBgNVHQ8BAf8EBAMCAYYw
DQYJKoZIhvcNAQELBQADggEBAI8dvj/5rTK4NxC6cNeRi4wF8HDLHLEVbOHfxQDr
99aQmLqDL6rc9LbzRqtH8Pga606J0NsB4owyEiumYjOUyPOVyUYKrxKt5Wj/0C3V
/sHKOdaRS+yT6O8XcsTddxbl9cIw6WroTRFvqnAtiaOt3JMCmU2rXjYa8w5tz/1t
gTwmDuN5u4+N+zfoK0Cc2hvMJdiYFhzPYbie1ffmcHXJTNPqUg9K2lfqDCmZ+xIA
PpVsaCU9401qPWRWftXJgb3vIVOsYB6l3KYYKdOpudaCzSbZVROmC4a693/E5hWM
nc8BTncWI0KGWIzTQasuSEye50R6gc9wZCGIElmhWcu3NYk=
-----END CERTIFICATE-----
"#
                .to_owned(),
                secret_name: "luks_passphrase".to_owned(),
            }),
        };

        assert_eq!(config, expected);
        Ok(())
    }
}
