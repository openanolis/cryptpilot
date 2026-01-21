use anyhow::{bail, Context, Result};
use clap::{command, Parser, ValueEnum};
use cryptpilot::{
    config::encrypt::{EncryptConfig, KeyProviderConfig},
    provider::{
        exec::ExecConfig,
        kbs::{CdhType, KbsConfig},
        kms::KmsConfig,
        oidc::{AliyunKmsConfig, Kms, OidcConfig},
        otp::OtpConfig,
    },
    types::MakeFsType,
};
use documented::{Documented, DocumentedFields};
use serde::{Deserialize, Serialize};
use shadow_rs::shadow;
use toml_edit::{Decor, DocumentMut, RawString, Table};

shadow!(build);

use crate::build::CLAP_LONG_VERSION;

// Volume configuration structures
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

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Args {
    #[clap(long, short = 't')]
    #[arg(value_enum)]
    /// The type of volume to generate.
    volume_type: VolumeType,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum VolumeType {
    Otp,
    Kms,
    Kbs,
    Oidc,
    Exec,
}

impl VolumeType {
    pub fn get_volume_config(self) -> VolumeConfig {
        let key_provider = match self {
            VolumeType::Otp => KeyProviderConfig::Otp(OtpConfig {}),
            VolumeType::Kms => KeyProviderConfig::Kms(KmsConfig {
                secret_name: "XXXXXXXXX".into(),
                client_key: r#"{
  "KeyId": "KAAP.XXXXXXXXX",
  "PrivateKeyData": "XXXXXXXXX"
}"#
                .into(),
                client_key_password: "XXXXXXXXX".into(),
                kms_instance_id: "kst-XXXXXXXXX".into(),
                kms_cert_pem: r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                .into(),
            }),
            VolumeType::Kbs => KeyProviderConfig::Kbs(KbsConfig {
                cdh_type: CdhType::OneShot {
                    kbs_url: "https://1.2.3.4:8080".into(),
                    kbs_root_cert: Some(
                        r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                        .into(),
                    ),
                },
                key_uri: "kbs:///default/mykey/volume_data0".into(),
            }),
            VolumeType::Oidc => KeyProviderConfig::Oidc(OidcConfig {
                kms: Kms::Aliyun(AliyunKmsConfig {
                    oidc_provider_arn: "acs:ram::113511544585:oidc-provider/TestOidcIdp".into(),
                    role_arn: "acs:ram::113511544585:role/testoidc".into(),
                    region_id: "cn-beijing".into(),
                }),
                command: "some-cli".into(),
                args: vec!["-c".into(), "/etc/config.json".into(), "get-token".into()],
                key_id: "disk-decryption-key".into(),
            }),
            VolumeType::Exec => KeyProviderConfig::Exec(ExecConfig {
                command: "echo".into(),
                args: vec!["passphrase".into()],
            }),
        };
        VolumeConfig {
            dev: "/dev/nvme1n1p1".into(),
            volume: "data0".into(),
            extra_config: ExtraConfig {
                auto_open: Some(true),
                makefs: Some(MakeFsType::Ext4),
                integrity: Some(true),
            },
            encrypt: EncryptConfig { key_provider },
        }
    }
}

trait AsAnnotatedToml {
    fn as_annotated_toml(&self) -> Result<DocumentMut>;
}

impl AsAnnotatedToml for VolumeConfig {
    fn as_annotated_toml(&self) -> Result<DocumentMut> {
        let mut toml = toml_edit::ser::to_string_pretty(self)?.parse::<DocumentMut>()?;

        annotate_toml_table::<VolumeConfig>(toml.as_table_mut())
            .context("Failed to annotate `VolumeConfig`")?;
        annotate_toml_table::<ExtraConfig>(toml.as_table_mut())
            .context("Failed to annotate `ExtraConfig`")?;

        let Some(key_provider) = toml.get_mut("encrypt") else {
            return Ok(toml);
        };
        let Some(key_provider) = key_provider.as_table_mut() else {
            return Ok(toml);
        };

        match self.encrypt.key_provider {
            KeyProviderConfig::Otp(_) => {
                let Some(provider_config) = key_provider.get_mut("otp") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), OtpConfig::DOCS);
                annotate_toml_table::<OtpConfig>(provider_config)
                    .context("Failed to annotate `OtpConfig`")?;
            }
            KeyProviderConfig::Kms(_) => {
                let Some(provider_config) = key_provider.get_mut("kms") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), KmsConfig::DOCS);
                annotate_toml_table::<KmsConfig>(provider_config)
                    .context("Failed to annotate `KmsConfig`")?;
            }
            KeyProviderConfig::Kbs(_) => {
                let Some(provider_config) = key_provider.get_mut("kbs") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), KbsConfig::DOCS);
                annotate_toml_table::<KbsConfig>(provider_config)
                    .context("Failed to annotate `KbsConfig`")?;
            }
            KeyProviderConfig::Oidc(_) => {
                let Some(provider_config) = key_provider.get_mut("oidc") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), OidcConfig::DOCS);
                annotate_toml_table::<OidcConfig>(provider_config)
                    .context("Failed to annotate `OidcConfig`")?;

                let Some(kms_config) = provider_config.get_mut("kms") else {
                    return Ok(toml);
                };
                let Some(kms_config) = kms_config.as_table_mut() else {
                    return Ok(toml);
                };

                append_docs_as_toml_comments(kms_config.decor_mut(), AliyunKmsConfig::DOCS);
                annotate_toml_table::<AliyunKmsConfig>(kms_config)
                    .context("Failed to annotate `AliyunKmsConfig`")?;
            }
            KeyProviderConfig::Exec(_) => {
                let Some(provider_config) = key_provider.get_mut("exec") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), ExecConfig::DOCS);
                annotate_toml_table::<ExecConfig>(provider_config)
                    .context("Failed to annotate `ExecConfig`")?;
            }
            _ => {}
        }

        annotate_toml_table::<ExtraConfig>(key_provider)
            .context("Failed to annotate `ExtraConfig`")?;
        Ok(toml)
    }
}

fn append_docs_as_toml_comments(decor: &mut Decor, docs: &str) {
    let old_prefix = decor.prefix().and_then(RawString::as_str);
    let last_line = old_prefix.and_then(|prefix| prefix.lines().last());

    let comments = docs
        .lines()
        .map(|l| {
            if l.is_empty() {
                "#\n".into()
            } else {
                format!("# {l}\n")
            }
        })
        .collect();

    let new_prefix = match (old_prefix, last_line) {
        (None | Some(""), None) => comments,
        (None, Some(_)) => unreachable!(),
        (Some(_), None) => unreachable!(),
        (Some(prefix), Some("")) => format!("{prefix}{comments}"),
        (Some(prefix), Some(_)) => format!("{prefix}#\n{comments}"),
    };
    decor.set_prefix(new_prefix);
}

fn annotate_toml_table<T>(table: &mut Table) -> Result<()>
where
    T: DocumentedFields,
{
    use toml_edit::Item as I;

    for (mut key, value) in table.iter_mut() {
        let field_name = key.get();
        let Ok(docs) = T::get_field_docs(field_name) else {
            continue;
        };

        match value {
            I::None => bail!("Encountered a `None` key unexpectedly"),
            I::Value(_) => append_docs_as_toml_comments(key.leaf_decor_mut(), docs),
            I::Table(sub_table) => append_docs_as_toml_comments(sub_table.decor_mut(), docs),
            I::ArrayOfTables(array) => {
                let first_table = array
                    .iter_mut()
                    .next()
                    .context("Array of table should not be empty")?;
                append_docs_as_toml_comments(first_table.decor_mut(), docs);
            }
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();

    let doc = args.volume_type.get_volume_config().as_annotated_toml()?;

    print!("{}", doc);
    Ok(())
}
