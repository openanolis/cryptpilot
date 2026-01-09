use documented::DocumentedFields;
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

use cryptpilot::{config::encrypt::EncryptConfig, types::MakeFsType};

/// The volume configuration.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct VolumeConfig {
    /// The name of resulting volume with decrypted data, which will be set up below `/dev/mapper/`.
    pub volume: String,

    /// The path to the underlying encrypted device.
    pub dev: String,

    /// Extra configuration for the volume.
    #[serde(flatten)]
    pub extra_config: ExtraConfig,

    /// The encryption specific configurations
    pub encrypt: EncryptConfig,
}

impl VolumeConfig {
    pub fn volume_path(&self) -> PathBuf {
        Path::new("/dev/mapper").join(&self.volume)
    }
}

/// Extra configuration for the volume.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct ExtraConfig {
    /// Whether or not to open the LUKS2 device and set up mapping during booting. The default value is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_open: Option<bool>,

    /// The file system to initialize on the volume. Allowed values are ["swap", "ext4", "xfs", "vfat"]. If is not specified, or the device is not "empty", i.e. it contains any signature, the operation will be skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub makefs: Option<MakeFsType>,

    /// Whether or not to enable support for data integrity. The default value is false. Note that integrity cannot prevent a replay (rollback) attack.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrity: Option<bool>,
}

#[cfg(test)]
pub mod tests {

    use cryptpilot::{
        config::encrypt::KeyProviderConfig,
        provider::oidc::{AliyunKmsConfig, Kms, OidcConfig},
        types::MakeFsType,
    };

    #[allow(unused_imports)]
    use super::*;
    use anyhow::Result;

    #[test]
    fn test_deserialize_otp() -> Result<()> {
        let raw = r#"
        dev = "/dev/nvme1n1p1"
        volume = "data"

        [encrypt.otp]
        "#;

        let config: VolumeConfig = toml::from_str(raw)?;
        assert_eq!(
            config,
            VolumeConfig {
                volume: "data".into(),
                dev: "/dev/nvme1n1p1".into(),
                extra_config: ExtraConfig {
                    auto_open: None,
                    makefs: None,
                    integrity: None,
                },
                encrypt: EncryptConfig {
                    key_provider: KeyProviderConfig::Otp(cryptpilot::provider::otp::OtpConfig {}),
                }
            }
        );
        Ok(())
    }

    #[test]
    fn test_deserialize_kms() -> Result<()> {
        let raw = r#"
dev = "/dev/nvme1n1p2"
volume = "data1"

[encrypt.kms]
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
            dev: "/dev/nvme1n1p2".into(),
            extra_config: ExtraConfig {
                auto_open: None,
                makefs: None,
                integrity: None,
            },
            encrypt: EncryptConfig {
                key_provider: KeyProviderConfig::Kms(cryptpilot::provider::kms::KmsConfig {
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
            },
        };

        assert_eq!(config, expected);
        Ok(())
    }

    #[test]
    fn test_deserialize_oidc_test() {
        let raw = r#"
            volume = "data5"
            dev = "/dev/nvme1n1p6"
            auto_open = true
            makefs = "ext4"
            integrity = true

            [encrypt.oidc]
            command = "some-cli"
            args = [
                "-c",
                "/etc/config.json",
                "get-token",
            ]
            key_id = "disk-decryption-key"

            [encrypt.oidc.kms]
            type = "aliyun"
            oidc_provider_arn = "acs:ram::113511544585:oidc-provider/TestOidcIdp"
            role_arn = "acs:ram::113511544585:role/testoidc"
            region_id = "cn-beijing"
        "#;
        let config: VolumeConfig = toml::from_str(raw).unwrap();
        let expected = VolumeConfig {
            volume: "data5".into(),
            dev: "/dev/nvme1n1p6".into(),
            extra_config: ExtraConfig {
                auto_open: Some(true),
                makefs: Some(MakeFsType::Ext4),
                integrity: Some(true),
            },
            encrypt: EncryptConfig {
                key_provider: KeyProviderConfig::Oidc(OidcConfig {
                    kms: Kms::Aliyun(AliyunKmsConfig {
                        oidc_provider_arn: "acs:ram::113511544585:oidc-provider/TestOidcIdp".into(),
                        role_arn: "acs:ram::113511544585:role/testoidc".into(),
                        region_id: "cn-beijing".into(),
                    }),
                    command: "some-cli".into(),
                    args: vec!["-c".into(), "/etc/config.json".into(), "get-token".into()],
                    key_id: "disk-decryption-key".into(),
                }),
            },
        };
        assert_eq!(expected, config);
    }
}
