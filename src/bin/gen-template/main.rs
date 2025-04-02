use anyhow::{bail, Context, Result};
use clap::{command, Parser, ValueEnum};
use cryptpilot::{
    config::{
        encrypt::{EncryptConfig, KeyProviderConfig},
        fde::{DataConfig, FdeConfig, RootFsConfig, RwOverlayType},
        global::{BootServiceConfig, GlobalConfig},
        volume::{ExtraConfig, MakeFsType, VolumeConfig},
    },
    provider::{
        kbs::KbsConfig,
        kms::KmsConfig,
        oidc::{Kms, OidcConfig},
        otp::OtpConfig,
        tpm2::Tpm2Config,
        exec::ExecConfig,
    },
};
use documented::{Documented, DocumentedFields};
use shadow_rs::shadow;
use toml_edit::{Decor, DocumentMut, RawString, Table};

shadow!(build);

use crate::build::CLAP_LONG_VERSION;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Args {
    #[command(subcommand)]
    pub template: TemplateType,
}

#[derive(Parser, Debug)]
pub enum TemplateType {
    Volume {
        #[clap(long, short = 't')]
        #[arg(value_enum)]
        /// The type of volume to generate.
        volume_type: VolumeType,
    },
    Global,
    Fde,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum VolumeType {
    Otp,
    Kms,
    Kbs,
    ZeroTrust,
    Exec,
}

impl VolumeType {
    pub fn get_volume_config(self) -> VolumeConfig {
        let mut volume_config = VolumeConfig {
            dev: "/dev/nvme1n1p1".into(),
            volume: "data0".into(),
            extra_config: ExtraConfig {
                auto_open: Some(true),
                makefs: Some(MakeFsType::Ext4),
                integrity: Some(true),
            },
            encrypt: EncryptConfig {
                key_provider: KeyProviderConfig::Otp(OtpConfig {}),
            },
        };

        match self {
            VolumeType::Otp => { // Do nothing
            }
            VolumeType::Kms => {
                volume_config.encrypt.key_provider = KeyProviderConfig::Kms(KmsConfig {
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
                });
            }
            VolumeType::Kbs => {
                volume_config.encrypt.key_provider = KeyProviderConfig::Kbs(KbsConfig {
                    kbs_url: "https://1.2.3.4:8080".into(),
                    key_uri: "kbs:///default/mykey/volume_data0".into(),
                    kbs_root_cert: Some(
                        r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                        .into(),
                    ),
                });
            }
            VolumeType::ZeroTrust => {
                volume_config.encrypt.key_provider = KeyProviderConfig::Oidc(OidcConfig {
                    kms: Kms::Aliyun {
                        oidc_provider_arn: "acs:ram::113511544585:oidc-provider/TestOidcIdp".into(),
                        role_arn: "acs:ram::113511544585:role/testoidc".into(),
                        region_id: "cn-beijing".into(),
                    },
                    command: "some-cli".into(),
                    args: vec!["-c".into(), "/etc/config.json".into(), "get-token".into()],
                    key_id: "disk-decryption-key".into(),
                })
            }
            VolumeType::Exec => {
                volume_config.encrypt.key_provider = KeyProviderConfig::Exec(ExecConfig {
                    command: "echo".into(),
                    args: vec!["passphrase".into()],
                })
            }
        };
        volume_config
    }
}

trait AsAnnotatedToml {
    fn as_annotated_toml(&self) -> Result<DocumentMut>;
}

impl AsAnnotatedToml for VolumeConfig {
    fn as_annotated_toml(&self) -> Result<DocumentMut> {
        let mut toml = toml_edit::ser::to_string_pretty(self)?.parse::<DocumentMut>()?;

        // annotate `Config`
        annotate_toml_table::<VolumeConfig>(toml.as_table_mut())
            .context("Failed to annotate `VolumeConfig`")?;
        annotate_toml_table::<ExtraConfig>(toml.as_table_mut())
            .context("Failed to annotate `ExtraConfig`")?;

        let Some(key_provider) = toml.get_mut("encrypt") else {
            // Return if there is no key_provider
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
                    .context("Failed to annotate `OtpOptions`")?;
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
                    .context("Failed to annotate `KmsOptions`")?;
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
                    .context("Failed to annotate `KbsOptions`")?;
            }
            KeyProviderConfig::Tpm2(_) => {
                let Some(provider_config) = key_provider.get_mut("tpm") else {
                    return Ok(toml);
                };
                let Some(provider_config) = provider_config.as_table_mut() else {
                    return Ok(toml);
                };
                append_docs_as_toml_comments(provider_config.decor_mut(), Tpm2Config::DOCS);
                annotate_toml_table::<Tpm2Config>(provider_config)
                    .context("Failed to annotate `Tpm2Options`")?;
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
                    .context("Failed to annotate `ZeroTrustOptions`")?;
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
                    .context("Failed to annotate `ExecOptions`")?;
            }
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
        // no prior comments
        (None | Some(""), None) => comments,
        // no prior comments, but somehow there are lines
        (None, Some(_)) => unreachable!(),
        // prior comments is contentful, but there are no lines
        (Some(_), None) => unreachable!(),
        // last line of prior comments is empty
        (Some(prefix), Some("")) => format!("{prefix}{comments}"),
        // last line of prior comments is contentful
        (Some(prefix), Some(_)) => format!("{prefix}#\n{comments}"),
    };
    decor.set_prefix(new_prefix);
}

/// From https://github.com/cyqsimon/openvpn-cred-management/blob/e040b32cebb5ecf361a5549feb3e3d5e22741913/src/config.rs#L246
///
/// Insert annotations as comments into the serialised TOML representation of a
/// type using its doc comments.
///
/// Note that this function is not recursive. We do not descend into sub-tables
/// and sub-arrays-of-tables and annotate their fields; we only annotate the
/// sub-table or sub-arrays-of-tables themselves with the doc comments of their
/// corresponding fields on this type.
fn annotate_toml_table<T>(table: &mut Table) -> Result<()>
where
    T: DocumentedFields,
{
    use toml_edit::Item as I;

    // docs on fields
    for (mut key, value) in table.iter_mut() {
        // extract docs
        let field_name = key.get();
        let Ok(docs) = T::get_field_docs(&field_name) else {
            // ignore fields not known to `T`
            continue;
        };

        // add comments
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

impl AsAnnotatedToml for GlobalConfig {
    fn as_annotated_toml(&self) -> Result<DocumentMut> {
        let mut toml = toml_edit::ser::to_string_pretty(self)?.parse::<DocumentMut>()?;
        annotate_toml_table::<GlobalConfig>(toml.as_table_mut())
            .context("Failed to annotate `GlobalConfig`")?;
        if let Some(item) = toml.get_mut("boot").and_then(|item| item.as_table_mut()) {
            annotate_toml_table::<BootServiceConfig>(item)
                .context("Failed to annotate `BootServiceConfig`")?;
        };

        Ok(toml)
    }
}

pub fn get_global_config() -> GlobalConfig {
    GlobalConfig {
        boot: Some(BootServiceConfig { verbose: false }),
    }
}

impl AsAnnotatedToml for FdeConfig {
    fn as_annotated_toml(&self) -> Result<DocumentMut> {
        let mut toml = toml_edit::ser::to_string_pretty(self)?.parse::<DocumentMut>()?;
        annotate_toml_table::<FdeConfig>(toml.as_table_mut())
            .context("Failed to annotate `FdeConfig`")?;
        if let Some(item) = toml.get_mut("rootfs").and_then(|item| item.as_table_mut()) {
            annotate_toml_table::<RootFsConfig>(item)
                .context("Failed to annotate `RootFsConfig`")?;
        };
        if let Some(item) = toml.get_mut("data").and_then(|item| item.as_table_mut()) {
            annotate_toml_table::<DataConfig>(item).context("Failed to annotate `DataConfig`")?;
        };
        Ok(toml)
    }
}

pub fn get_fde_config() -> FdeConfig {
    FdeConfig {
        rootfs: RootFsConfig {
            rw_overlay: Some(RwOverlayType::Disk),
            encrypt: Some(EncryptConfig {
                key_provider: KeyProviderConfig::Kbs(KbsConfig {
                    kbs_url: "https://1.2.3.4:8080".into(),
                    key_uri: "kbs:///default/mykey/rootfs_partition".into(),
                    kbs_root_cert: Some(
                        r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                        .into(),
                    ),
                }),
            }),
        },
        data: DataConfig {
            integrity: true,
            encrypt: EncryptConfig {
                key_provider: KeyProviderConfig::Kbs(KbsConfig {
                    kbs_url: "https://1.2.3.4:8080".into(),
                    key_uri: "kbs:///default/mykey/data_partition".into(),
                    kbs_root_cert: Some(
                        r#"-----BEGIN CERTIFICATE-----
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
-----END CERTIFICATE-----
"#
                        .into(),
                    ),
                }),
            },
        },
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let doc = match args.template {
        TemplateType::Volume { volume_type } => {
            volume_type.get_volume_config().as_annotated_toml()?
        }
        TemplateType::Global => get_global_config().as_annotated_toml()?,
        TemplateType::Fde => get_fde_config().as_annotated_toml()?,
    };

    print!("{}", doc.to_string());
    Ok(())
}
