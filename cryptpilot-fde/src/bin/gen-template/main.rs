use anyhow::{Context, Result};
use clap::{command, Parser};
use cryptpilot::config::encrypt::{EncryptConfig, KeyProviderConfig};
use cryptpilot::provider::kbs::KbsConfig;
use documented::DocumentedFields;
use serde::{Deserialize, Serialize};
use shadow_rs::shadow;
use toml_edit::{Decor, DocumentMut, RawString, Table};

shadow!(build);

use crate::build::CLAP_LONG_VERSION;

// FDE Configuration structures
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
    /// The type of read-write overlay layer over the underhood read-only rootfs. Can be "disk" or "ram". Default value is "disk".
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
#[serde(deny_unknown_fields)]
pub enum RwOverlayType {
    /// The overlay will be placed on disk, and be persistent.
    Disk,
    /// The overlay will be placed on tmpfs (in RAM), and be temporary.
    Ram,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    /// Configuration related to cryptpilot boot service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot: Option<BootServiceConfig>,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, DocumentedFields)]
#[serde(deny_unknown_fields)]
pub struct BootServiceConfig {
    /// Enable this option if you want to see more log when running cryptpilot boot service in initrd stage and in system stage.
    #[serde(default = "Default::default")]
    pub verbose: bool,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[clap(long_version = CLAP_LONG_VERSION)]
pub struct Args {
    #[command(subcommand)]
    pub template: TemplateType,
}

#[derive(Parser, Debug)]
pub enum TemplateType {
    Global,
    Fde,
}

trait AsAnnotatedToml {
    fn as_annotated_toml(&self) -> Result<DocumentMut>;
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
    use anyhow::bail;
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
        TemplateType::Global => get_global_config().as_annotated_toml()?,
        TemplateType::Fde => get_fde_config().as_annotated_toml()?,
    };

    print!("{}", doc);
    Ok(())
}
